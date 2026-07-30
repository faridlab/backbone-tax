//! The billing → tax invoice seam.
//!
//! Proves a posted billing invoice flows into the tax audit mirror through the module's PUBLIC
//! API: billing posts → emits an enriched `SalesInvoicePosted` / `PurchaseInvoicePosted` (carrying
//! the tax breakdown) → this test's ACL translates it to `PostedForTax` →
//! `TaxModule.efaktur_service.record_tax_transaction` records a `TaxTransaction` (and assigns a
//! gapless e-Faktur number for sales). Mirrors `backbone-billing/tests/ap_seam.rs` and
//! `backbone-selling/tests/invoice_seam.rs`.
//!
//! Dev-dep on backbone-billing ONLY — zero normal cargo edge (tax never imports billing; the
//! composition/ACL bridges them). Requires DATABASE_URL (:5433) with the `tax` and `billing`
//! schemas migrated (each migration `CREATE SCHEMA IF NOT EXISTS`; migrate externally, like the
//! other seam tests).

use std::sync::{Arc, Mutex};

use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use backbone_billing::application::service::billing_events::{
    BillingEvent, BillingEventSink, PurchaseInvoicePosted, SalesInvoicePosted,
};
use backbone_billing::application::service::billing_gl::{
    AccountingPostEnvelope, GlPostAck, GlPostRejected, GlPostSink,
};
use backbone_billing::application::service::billing_write_service::{
    BillingWriteService, NewInvoiceLine, NewPurchaseInvoice, NewSalesInvoice, NewTaxLine,
};

use backbone_tax::{PostedForTax, TaxModule};

fn d(s: &str) -> Decimal { Decimal::from_str_exact(s).unwrap() }
fn day() -> chrono::NaiveDate { chrono::NaiveDate::from_ymd_opt(2026, 7, 5).unwrap() }
fn uq(p: &str) -> String { format!("{p}-{}", &Uuid::new_v4().simple().to_string()[..8]) }

async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5433/backbone_tax".to_string());
    PgPool::connect(&url).await.expect("connect DB")
}

/// Records billing domain events so the test can route them → tax (the in-test composition ACL).
#[derive(Default, Clone)]
struct Recorder { events: Arc<Mutex<Vec<BillingEvent>>> }
impl BillingEventSink for Recorder {
    fn publish(&self, e: BillingEvent) { self.events.lock().unwrap().push(e); }
}

/// Fake GL sink — acks every post. The seam under test is billing→tax, not the GL, so a real
/// accounting post is out of scope (cf. backbone-billing/tests/schedules_and_events.rs::OkGl).
#[derive(Clone)]
struct OkGl { journal: Uuid, post: Uuid }
#[async_trait::async_trait]
impl GlPostSink for OkGl {
    async fn post(&self, _e: &AccountingPostEnvelope) -> Result<GlPostAck, GlPostRejected> {
        Ok(GlPostAck { post_id: self.post, journal_id: self.journal, idempotent_reuse: false })
    }
}

/// The composition ACL: translate billing's enriched `SalesInvoicePosted` → tax's `PostedForTax`.
/// This is the ONLY place billing and tax types meet — tax itself has zero cargo edges to billing.
fn sales_to_posted_for_tax(p: &SalesInvoicePosted) -> PostedForTax {
    PostedForTax {
        invoice_ref: p.invoice_id,
        company_id: p.company_id,
        invoice_kind: "sales".into(),
        posting_date: p.posting_date,
        taxable_base: p.taxable_base,
        output_total: p.output_total,
        input_total: Decimal::ZERO,
        withholding_total: Decimal::ZERO,
    }
}

fn purchase_to_posted_for_tax(p: &PurchaseInvoicePosted) -> PostedForTax {
    PostedForTax {
        invoice_ref: p.invoice_id,
        company_id: p.company_id,
        invoice_kind: "purchase".into(),
        posting_date: p.posting_date,
        taxable_base: p.taxable_base,
        output_total: Decimal::ZERO,
        input_total: p.input_total,
        withholding_total: p.withholding_total,
    }
}

// BT-1: a posted SALES invoice routes to tax → records a TaxTransaction + assigns a gapless
// e-Faktur number (010.NNN-NN.YYYYYYYY); re-routing the same event is idempotent.
#[tokio::test]
async fn sales_post_routes_to_tax_and_assigns_gapless_efaktur() {
    let pool = pool().await;
    let rec = Recorder::default();
    let bill = BillingWriteService::with_sink(pool.clone(), Arc::new(rec.clone()));
    let company = Uuid::new_v4();

    // 1) A sales invoice: 1,000,000 net + 110,000 PPN output (11%).
    let (item, rev, ar, ppn) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let inv = bill.create_sales_invoice(NewSalesInvoice {
        invoice_number: uq("SI"), company_id: company, branch_id: None, customer_id: Uuid::new_v4(),
        source_so_id: None, posting_date: day(), due_date: None, currency: None, receivable_account_id: ar,
        lines: vec![NewInvoiceLine { item_id: item, account_id: rev, description: None,
            quantity: d("1"), unit_price: d("1000000") }],
        tax_lines: vec![NewTaxLine { account_id: ppn, basis: "output".into(), description: None,
            rate: d("0.11"), tax_amount: d("110000") }],
    }).await.unwrap();

    // 2) Post (fake GL acks) → billing emits the enriched SalesInvoicePosted.
    bill.post_sales_invoice(inv, &OkGl { journal: Uuid::new_v4(), post: Uuid::new_v4() }).await.unwrap();
    let posted = rec.events.lock().unwrap().iter().find_map(|e| match e {
        BillingEvent::SalesInvoicePosted(p) if p.invoice_id == inv => Some(p.clone()), _ => None,
    }).expect("SalesInvoicePosted emitted");
    assert_eq!(posted.taxable_base, d("1000000.00"), "enriched event carries the net base");
    assert_eq!(posted.output_total, d("110000.00"), "enriched event carries PPN output");

    // 3) ACL → PostedForTax → tax records via the PUBLIC TaxModule.efaktur_service.
    let tax = TaxModule::builder().with_database(pool.clone()).build().unwrap();
    let pft = sales_to_posted_for_tax(&posted);
    let (txn, efaktur) = tax.efaktur_service.record_tax_transaction(&pft).await.unwrap();
    let efaktur = efaktur.expect("sales with output → e-Faktur assigned");

    // 4) TaxTransaction recorded; e-Faktur number is gapless DJP format.
    let txn_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tax.tax_transactions WHERE invoice_ref=$1 AND company_id=$2")
        .bind(inv).bind(company).fetch_one(&pool).await.unwrap();
    assert_eq!(txn_count, 1, "exactly one TaxTransaction for this invoice");
    let number: String = sqlx::query_scalar(
        "SELECT number FROM tax.efaktur_documents WHERE tax_transaction_id=$1")
        .bind(txn).fetch_one(&pool).await.unwrap();
    assert!(number.starts_with("010."), "transaction_code = 010 (standard VAT)");
    assert_eq!(number.len(), 19, "FFF.NNN-NN.YYYYYYYY = 19 chars");
    let _ = efaktur;

    // 5) Idempotency: re-route the same event → reuses the transaction, no duplicate e-Faktur.
    let (txn2, efaktur2) = tax.efaktur_service.record_tax_transaction(&pft).await.unwrap();
    assert_eq!(txn, txn2, "re-delivery reuses the same TaxTransaction");
    assert_eq!(efaktur, efaktur2.unwrap(), "re-delivery reuses the same EFakturDocument");
    let efaktur_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tax.efaktur_documents WHERE company_id=$1")
        .bind(company).fetch_one(&pool).await.unwrap();
    assert_eq!(efaktur_count, 1, "idempotent — still one e-Faktur");
}

// BT-2: a posted PURCHASE invoice routes to tax → records a TaxTransaction, but NO e-Faktur
// (only sales are numbered).
#[tokio::test]
async fn purchase_post_routes_to_tax_without_efaktur() {
    let pool = pool().await;
    let rec = Recorder::default();
    let bill = BillingWriteService::with_sink(pool.clone(), Arc::new(rec.clone()));
    let company = Uuid::new_v4();

    let (item, exp, ap, ppn_in) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let inv = bill.create_purchase_invoice(NewPurchaseInvoice {
        invoice_number: uq("PI"), company_id: company, branch_id: None, supplier_id: Uuid::new_v4(),
        source_po_id: None, posting_date: day(), due_date: None, currency: None, payable_account_id: ap,
        lines: vec![NewInvoiceLine { item_id: item, account_id: exp, description: None,
            quantity: d("1"), unit_price: d("800000") }],
        tax_lines: vec![NewTaxLine { account_id: ppn_in, basis: "input".into(), description: None,
            rate: d("0.11"), tax_amount: d("88000") }],
    }).await.unwrap();

    bill.post_purchase_invoice(inv, &OkGl { journal: Uuid::new_v4(), post: Uuid::new_v4() }).await.unwrap();
    let posted = rec.events.lock().unwrap().iter().find_map(|e| match e {
        BillingEvent::PurchaseInvoicePosted(p) if p.invoice_id == inv => Some(p.clone()), _ => None,
    }).expect("PurchaseInvoicePosted emitted");
    assert_eq!(posted.taxable_base, d("800000.00"));
    assert_eq!(posted.input_total, d("88000.00"));

    let tax = TaxModule::builder().with_database(pool.clone()).build().unwrap();
    let (_txn, efaktur) = tax.efaktur_service
        .record_tax_transaction(&purchase_to_posted_for_tax(&posted)).await.unwrap();
    assert!(efaktur.is_none(), "purchase → no e-Faktur assigned");
    let txn_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tax.tax_transactions WHERE invoice_ref=$1 AND company_id=$2")
        .bind(inv).bind(company).fetch_one(&pool).await.unwrap();
    assert_eq!(txn_count, 1, "exactly one TaxTransaction for this purchase invoice");
}

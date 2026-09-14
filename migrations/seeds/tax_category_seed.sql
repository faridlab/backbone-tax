-- Indonesian tax categories.
--
-- The vocabulary a document's tax lines are classified under. These are the
-- statutory tax types in Indonesia, not a tenant preference, which is why the
-- table is shared-class: one list every scope reads, while a company may still
-- add its own on its own unit.
--
-- PPN is the value-added tax; PPnBM the luxury-goods sales tax that rides on
-- top of it for certain goods. The withholding kinds mirror the articles of the
-- income-tax law that the withholding categories carry rates for.

INSERT INTO tax.tax_categories (id, code, name, tax_kind, status, metadata) VALUES
  ('7b000000-0000-4a70-8000-000000000001', 'PPN',     'PPN — Pajak Pertambahan Nilai',            'vat',         'active', '{}'::jsonb),
  ('7b000000-0000-4a70-8000-000000000002', 'PPNBM',   'PPnBM — Pajak Penjualan atas Barang Mewah','sales',       'active', '{}'::jsonb),
  ('7b000000-0000-4a70-8000-000000000003', 'PPH21',   'PPh 21 — Penghasilan Orang Pribadi',       'withholding', 'active', '{"pasal":"21"}'::jsonb),
  ('7b000000-0000-4a70-8000-000000000004', 'PPH22',   'PPh 22 — Pemungutan atas Perdagangan',     'withholding', 'active', '{"pasal":"22"}'::jsonb),
  ('7b000000-0000-4a70-8000-000000000005', 'PPH23',   'PPh 23 — Modal, Jasa dan Hadiah',          'withholding', 'active', '{"pasal":"23"}'::jsonb),
  ('7b000000-0000-4a70-8000-000000000006', 'PPH26',   'PPh 26 — Wajib Pajak Luar Negeri',         'withholding', 'active', '{"pasal":"26"}'::jsonb),
  ('7b000000-0000-4a70-8000-000000000007', 'PPH4A2',  'PPh Final Pasal 4 Ayat (2)',               'withholding', 'active', '{"pasal":"4(2)","final":true}'::jsonb),
  ('7b000000-0000-4a70-8000-000000000008', 'PPH15',   'PPh 15 — Norma Penghitungan Khusus',       'withholding', 'active', '{"pasal":"15"}'::jsonb),
  ('7b000000-0000-4a70-8000-000000000009', 'NONTAX',  'Tidak Dikenakan Pajak',                    'other',       'active', '{}'::jsonb)
ON CONFLICT (id) DO NOTHING;

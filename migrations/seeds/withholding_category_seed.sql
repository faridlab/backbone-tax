-- Indonesian withholding tax categories (PPh potong/pungut).
--
-- Rates are the statutory ones published by the Directorate General of Taxes.
-- They are national law, not a tenant preference, so they belong in reference
-- data rather than in each tenant's configuration.
--
-- Two things this file deliberately does NOT encode.
--
-- The doubled rate for a counterparty without an NPWP: the statute raises the
-- withholding by 100% in that case, but that is a property of the counterparty
-- at transaction time, not of the category. Encoding it as separate rows would
-- invite picking the wrong one.
--
-- `account_id` stays NULL. The GL account a withholding posts to belongs to a
-- company's own chart, created per company by the chart-install verb.
--
-- `effective_from` is set to the start of the current self-assessment era
-- rather than a guess at each rule's origin; `effective_to` NULL means in force.
-- A tenant supersedes a row by closing it and inserting the replacement, which
-- is why the table carries the dates at all.

INSERT INTO tax.withholding_categories
  (id, code, name, rate, threshold_amount, account_id, effective_from, effective_to, status, metadata)
VALUES
  -- Pasal 23 — capital, services, prizes
  ('7a000000-0000-4e50-8000-000000000001', 'PPH23-DIVIDEN',   'PPh 23 — Dividen',                          15.00, 0, NULL, '2009-01-01', NULL, 'active', '{"pasal":"23"}'::jsonb),
  ('7a000000-0000-4e50-8000-000000000002', 'PPH23-BUNGA',     'PPh 23 — Bunga',                            15.00, 0, NULL, '2009-01-01', NULL, 'active', '{"pasal":"23"}'::jsonb),
  ('7a000000-0000-4e50-8000-000000000003', 'PPH23-ROYALTI',   'PPh 23 — Royalti',                          15.00, 0, NULL, '2009-01-01', NULL, 'active', '{"pasal":"23"}'::jsonb),
  ('7a000000-0000-4e50-8000-000000000004', 'PPH23-HADIAH',    'PPh 23 — Hadiah dan Penghargaan',           15.00, 0, NULL, '2009-01-01', NULL, 'active', '{"pasal":"23"}'::jsonb),
  ('7a000000-0000-4e50-8000-000000000005', 'PPH23-SEWA',      'PPh 23 — Sewa dan Penggunaan Harta',         2.00, 0, NULL, '2009-01-01', NULL, 'active', '{"pasal":"23"}'::jsonb),
  ('7a000000-0000-4e50-8000-000000000006', 'PPH23-JASA',      'PPh 23 — Jasa Teknik, Manajemen, Konsultan', 2.00, 0, NULL, '2009-01-01', NULL, 'active', '{"pasal":"23"}'::jsonb),
  -- Pasal 26 — payments to non-residents
  ('7a000000-0000-4e50-8000-000000000010', 'PPH26',           'PPh 26 — Wajib Pajak Luar Negeri',          20.00, 0, NULL, '2009-01-01', NULL, 'active', '{"pasal":"26","note":"tax treaty may reduce"}'::jsonb),
  -- Pasal 4 ayat 2 — final
  ('7a000000-0000-4e50-8000-000000000020', 'PPH4A2-UMKM',     'PPh Final 4(2) — Peredaran Bruto Tertentu',  0.50, 0, NULL, '2018-07-01', NULL, 'active', '{"pasal":"4(2)","final":true}'::jsonb),
  ('7a000000-0000-4e50-8000-000000000021', 'PPH4A2-SEWA',     'PPh Final 4(2) — Sewa Tanah dan Bangunan',  10.00, 0, NULL, '2009-01-01', NULL, 'active', '{"pasal":"4(2)","final":true}'::jsonb),
  ('7a000000-0000-4e50-8000-000000000022', 'PPH4A2-ALIH',     'PPh Final 4(2) — Pengalihan Hak Tanah/Bangunan', 2.50, 0, NULL, '2016-09-01', NULL, 'active', '{"pasal":"4(2)","final":true}'::jsonb),
  ('7a000000-0000-4e50-8000-000000000023', 'PPH4A2-DEPOSITO','PPh Final 4(2) — Bunga Deposito dan Tabungan', 20.00, 0, NULL, '2009-01-01', NULL, 'active', '{"pasal":"4(2)","final":true}'::jsonb),
  ('7a000000-0000-4e50-8000-000000000024', 'PPH4A2-OBLIGASI','PPh Final 4(2) — Bunga/Diskonto Obligasi',   10.00, 0, NULL, '2009-01-01', NULL, 'active', '{"pasal":"4(2)","final":true}'::jsonb)
ON CONFLICT (id) DO NOTHING;

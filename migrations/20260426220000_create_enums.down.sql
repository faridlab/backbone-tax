-- Down: drop enum types for tax module
DROP TYPE IF EXISTS charge_type CASCADE;
DROP TYPE IF EXISTS template_type CASCADE;
DROP TYPE IF EXISTS tax_status CASCADE;
DROP TYPE IF EXISTS tax_kind CASCADE;

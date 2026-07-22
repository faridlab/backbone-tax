-- Down migration: reverse the tenant-scope tax migration (ADR-0010 B1).
-- Hand-authored (user-owned). Not regenerated.
--
-- Disarms the fence, drops per-company composite uniques + the company_id column, restores
-- the original global unique indexes and the original (pre-tenant) withholding EXCLUDE shape.
-- The tax_template_rows EXCLUDE was untouched by the up migration and is not touched here.

-- ==============================================================================
-- Step 1: Disarm fence — drop policy, unforce, disable, drop NOT NULL.
-- ==============================================================================

DO $$
DECLARE
    t    text;
    tabs text[] := ARRAY[
        'tax_categories','tax_templates','tax_template_rows','withholding_categories'
    ];
BEGIN
    FOREACH t IN ARRAY tabs LOOP
        EXECUTE format('DROP POLICY IF EXISTS %I ON tax.%I', t || '_company_isolation', t);
        EXECUTE format('ALTER TABLE IF EXISTS tax.%I NO FORCE ROW LEVEL SECURITY', t);
        EXECUTE format('ALTER TABLE IF EXISTS tax.%I DISABLE ROW LEVEL SECURITY', t);
        EXECUTE format('ALTER TABLE IF EXISTS tax.%I ALTER COLUMN company_id DROP NOT NULL', t);
    END LOOP;
END $$;

-- ==============================================================================
-- Step 2: Restore global unique indexes; drop per-company composites.
-- ==============================================================================

DROP INDEX IF EXISTS tax.idx_tax_categories_company_id_code;
DROP INDEX IF EXISTS tax.idx_tax_templates_company_id_code;

CREATE UNIQUE INDEX IF NOT EXISTS idx_tax_categories_code
    ON tax.tax_categories (code) WHERE (metadata ->> 'deleted_at') IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_tax_templates_code
    ON tax.tax_templates (code) WHERE (metadata ->> 'deleted_at') IS NULL;

-- Drop the supporting per-company indexes added by the up migration.
DROP INDEX IF EXISTS tax.idx_tax_categories_company_id;
DROP INDEX IF EXISTS tax.idx_tax_templates_company_id;
DROP INDEX IF EXISTS tax.idx_tax_template_rows_company_id;
DROP INDEX IF EXISTS tax.idx_withholding_categories_company_id;

-- ==============================================================================
-- Step 3: Restore the original (global) withholding EXCLUDE constraint.
-- ==============================================================================

ALTER TABLE tax.withholding_categories DROP CONSTRAINT IF EXISTS excl_withholding_no_overlap;
ALTER TABLE tax.withholding_categories
  ADD CONSTRAINT excl_withholding_no_overlap
  EXCLUDE USING gist (
    code WITH =,
    daterange(effective_from, COALESCE(effective_to, 'infinity'::date), '[]') WITH &&
  );

-- ==============================================================================
-- Step 4: Drop the company_id column on every tax table.
-- ==============================================================================

ALTER TABLE tax.tax_categories         DROP COLUMN IF EXISTS company_id;
ALTER TABLE tax.tax_templates          DROP COLUMN IF EXISTS company_id;
ALTER TABLE tax.tax_template_rows      DROP COLUMN IF EXISTS company_id;
ALTER TABLE tax.withholding_categories DROP COLUMN IF EXISTS company_id;

-- Migration: tenant-scope tax (ADR-0010 Decision B1)
-- Hand-authored (user-owned). Not regenerated.
--
-- Tax config (categories / templates / template-rows / withholding) was fully global — zero
-- company_id, zero RLS. This migration makes every tax entity tenant-scoped and fences each
-- table with the ADR-0008 RLS invariant:
--
--     company_id = NULLIF(current_setting('app.company_id', true), '')::uuid
--
-- Per-table shape: ADD COLUMN company_id UUID → backfill → SET NOT NULL →
-- ENABLE + FORCE RLS → POLICY. Global unique indexes (tax_categories.code, tax_templates.code)
-- are replaced with composite (company_id, code) per-company uniques, preserving the
-- `deleted_at IS NULL` soft-delete WHERE clause. The withholding_categories EXCLUDE constraint
-- (ADR-0002) is reshaped to include company_id so two tenants can reuse the same code without
-- an overlap collision; the tax_template_rows EXCLUDE (template_id, sort_order, effective_window)
-- is already tenant-correct because template_id is per-company, so it is left untouched.
--
-- BACKFILL + FENCE POLICY (ADR-0010 B1, resolved 2026-07-22 — mirrors catalog verbatim):
--   - If `organization.companies` has exactly one live row, backfill every tax row to it
--     (convenience for the single-company / dev / demo case).
--   - The RLS fence (NOT NULL + ENABLE + FORCE + POLICY) is then armed UNCONDITIONALLY
--     on every table that has zero NULL company_id rows.
--   - If ANY tax row still has NULL company_id after backfill (the multi-company or no-org
--     case with existing data), the migration FAILS LOUD — RAISE EXCEPTION naming every stray
--     table + row count — rather than silently leaving the fence disarmed. The operator must
--     assign those rows (or confirm a fresh DB), then re-run; the migration is idempotent and
--     will arm the fence once clean. We never pick an arbitrary company_id, and we never ship
--     a disarmed fence in the multi-tenant case where it is needed most.
--
-- No SQL FK to organization.companies is added: tax is a framework module and must stay
-- independently deployable. RLS is the fence, not the FK (matches catalog / pos).

-- ==============================================================================
-- Step 1: ADD COLUMN company_id UUID (nullable) on every tax table.
-- ==============================================================================

ALTER TABLE tax.tax_categories         ADD COLUMN IF NOT EXISTS company_id UUID;
ALTER TABLE tax.tax_templates          ADD COLUMN IF NOT EXISTS company_id UUID;
ALTER TABLE tax.tax_template_rows      ADD COLUMN IF NOT EXISTS company_id UUID;
ALTER TABLE tax.withholding_categories ADD COLUMN IF NOT EXISTS company_id UUID;

-- Supporting index for per-company queries (added unconditionally; cheap).
CREATE INDEX IF NOT EXISTS idx_tax_categories_company_id         ON tax.tax_categories (company_id);
CREATE INDEX IF NOT EXISTS idx_tax_templates_company_id          ON tax.tax_templates (company_id);
CREATE INDEX IF NOT EXISTS idx_tax_template_rows_company_id      ON tax.tax_template_rows (company_id);
CREATE INDEX IF NOT EXISTS idx_withholding_categories_company_id ON tax.withholding_categories (company_id);

-- ==============================================================================
-- Step 2: BACKFILL — only when exactly one live company exists (convenience).
-- Multi-company / no-org deployments skip backfill; Step 4 then fails loud on any
-- remaining NULL rows so the fence is never silently disarmed.
-- ==============================================================================

DO $$
DECLARE
    has_org boolean;
    cnt     int;
    cid     uuid;
    t       text;
    tabs    text[] := ARRAY[
        'tax_categories','tax_templates','tax_template_rows','withholding_categories'
    ];
BEGIN
    SELECT EXISTS (SELECT 1 FROM pg_namespace WHERE nspname = 'organization') INTO has_org;
    IF has_org THEN
        EXECUTE $q$
            SELECT COUNT(*) FROM organization.companies
            WHERE (metadata ->> 'deleted_at') IS NULL
        $q$ INTO cnt;
    ELSE
        -- organization schema not installed in this deployment → unresolvable.
        cnt := -1;
    END IF;

    IF cnt = 1 THEN
        EXECUTE $q$
            SELECT id FROM organization.companies
            WHERE (metadata ->> 'deleted_at') IS NULL
            LIMIT 1
        $q$ INTO cid;

        RAISE NOTICE 'tax ADR-0010 B1: exactly 1 company (%) — backfilling % tables', cid, array_length(tabs, 1);
        FOREACH t IN ARRAY tabs LOOP
            EXECUTE format('UPDATE tax.%I SET company_id = $1 WHERE company_id IS NULL', t)
                USING cid;
        END LOOP;
    ELSE
        -- AMBIGUOUS (0 or >1 companies, or no organization schema). Do NOT backfill and do NOT
        -- pick an arbitrary company. Step 4 will fail loud if any NULL rows remain.
        RAISE NOTICE
            'tax ADR-0010 B1: backfill skipped (organization.companies live-row count=%). '
            'Step 4 will fail loud on any tax rows still missing company_id.',
            cnt;
    END IF;
END $$;

-- ==============================================================================
-- Step 3: UNIQUE INDEX change — drop global, create per-company composite.
-- Reshape the withholding EXCLUDE to include company_id (preserves ADR-0002 within a tenant
-- while allowing two tenants to reuse the same code without an overlap collision).
-- (Always applied: the column is nullable-safe with the partial WHERE, and once backfill is
--  resolved later these indexes must already be per-company.)
-- ==============================================================================

-- ── tax_categories: code ─────────────────────────────────────────────────────
DROP INDEX IF EXISTS tax.idx_tax_categories_code;
CREATE UNIQUE INDEX IF NOT EXISTS idx_tax_categories_company_id_code
    ON tax.tax_categories (company_id, code)
    WHERE (metadata ->> 'deleted_at') IS NULL;

-- ── tax_templates: code ──────────────────────────────────────────────────────
DROP INDEX IF EXISTS tax.idx_tax_templates_code;
CREATE UNIQUE INDEX IF NOT EXISTS idx_tax_templates_company_id_code
    ON tax.tax_templates (company_id, code)
    WHERE (metadata ->> 'deleted_at') IS NULL;

-- ── withholding_categories: reshape the ADR-0002 EXCLUDE to be per-company ───
-- The original constraint forbade two rows of the same `code` from overlapping effective
-- windows. Under multi-tenant that must be scoped: two companies can now carry the same
-- code (e.g. each tenant's own PPh-23 category) without falsely colliding. The constraint
-- name is preserved so the original (2200020) down migration's DROP CONSTRAINT still works.
-- btree_gist already ships a GiST opclass for uuid; the extension was enabled by 2200020.
ALTER TABLE tax.withholding_categories DROP CONSTRAINT IF EXISTS excl_withholding_no_overlap;
ALTER TABLE tax.withholding_categories
  ADD CONSTRAINT excl_withholding_no_overlap
  EXCLUDE USING gist (
    company_id WITH =,
    code WITH =,
    daterange(effective_from, COALESCE(effective_to, 'infinity'::date), '[]') WITH &&
  );

-- ==============================================================================
-- Step 4: FAIL LOUD on strays, then arm the fence on ALL tables atomically.
-- First sweep counts NULL-company_id rows per table. If ANY exist, RAISE EXCEPTION listing
-- every stray table + count and abort — the fence is never partially armed and never silently
-- disarmed in the multi-tenant case. If all clean, arm all 4. Idempotent: after the operator
-- assigns strays, re-running arms the fence.
-- ==============================================================================

DO $$
DECLARE
    t            text;
    null_rows    int;
    tabs         text[] := ARRAY[
        'tax_categories','tax_templates','tax_template_rows','withholding_categories'
    ];
    strays       text[] := ARRAY[]::text[];
BEGIN
    -- Sweep: collect every table that still has unassigned rows.
    FOREACH t IN ARRAY tabs LOOP
        EXECUTE format('SELECT COUNT(*) FROM tax.%I WHERE company_id IS NULL', t) INTO null_rows;
        IF null_rows > 0 THEN
            strays := array_append(strays, format('%I=%s', t, null_rows));
        END IF;
    END LOOP;

    -- Fail loud if anything is unresolved — do NOT ship a disarmed fence.
    IF array_length(strays, 1) IS NOT NULL THEN
        RAISE EXCEPTION
            'tax ADR-0010 B1: refusing to fence — % tax table(s) still have NULL company_id (%). '
            'Assign every tax row to a tenant (or confirm a fresh DB), then re-run this migration. '
            'No RLS fence has been armed.',
            array_length(strays, 1), array_to_string(strays, ', ');
    END IF;

    -- All clean → arm the fence on every table.
    FOREACH t IN ARRAY tabs LOOP
        EXECUTE format('ALTER TABLE tax.%I ALTER COLUMN company_id SET NOT NULL', t);
        EXECUTE format('ALTER TABLE tax.%I ENABLE ROW LEVEL SECURITY', t);
        EXECUTE format('ALTER TABLE tax.%I FORCE  ROW LEVEL SECURITY', t);
        EXECUTE format(
            'DROP POLICY IF EXISTS %I ON tax.%I; '
            'CREATE POLICY %I ON tax.%I FOR ALL '
            'USING      (company_id = NULLIF(current_setting(''app.company_id'', true), '''')::uuid) '
            'WITH CHECK (company_id = NULLIF(current_setting(''app.company_id'', true), '''')::uuid)',
            t || '_company_isolation', t,
            t || '_company_isolation', t
        );
    END LOOP;

    RAISE NOTICE 'tax ADR-0010 B1: RLS fence live on all 4 tax tables.';
END $$;

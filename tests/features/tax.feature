# Tax acceptance oracle — backbone-tax
# Flow map:    docs/business-flows/tax.md
# Golden cases: docs/business-flows/golden-cases.md
# Executable truth: tests/tax_golden_cases.rs + tests/integrity_probes.rs

Feature: Compute tax lines for a taxable base
  In order to keep tax out of base invoice models
  As a producing module (billing, selling, buying)
  I want to ask the tax engine to compute tax lines that flow into an AccountingPost

  Background:
    Given the tenant schema "tax" is migrated

  @happy-path @module:tax @tgc-1
  Scenario: Exclusive VAT
    Given a sales template with an 11% row effective from 2022-04-01
    When I calculate tax on 1,000,000 as of 2026-07-03
    Then one tax line of 110,000 is returned

  @happy-path @module:tax @tgc-3
  Scenario: Effective-dated rate change
    Given a template with 11% until 2024-12-31 and 12% from 2025-01-01
    When I calculate on 1,000,000 as of 2024-06-01 and again as of 2025-06-01
    Then the tax is 110,000 then 120,000

  @withholding @module:tax @tgc-5
  Scenario: Withholding applies only above threshold
    Given a PPh 2% category with a 1,000,000 threshold
    When I resolve withholding on 5,000,000 and on 500,000
    Then I get a -100,000 line, then no line

  @validation @module:tax @igc-2
  Scenario: A row for a missing template is rejected
    When I add a template row for a non-existent template
    Then the request is rejected with "template_not_found"

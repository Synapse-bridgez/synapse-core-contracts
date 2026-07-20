# Upgrade Decision — Implementation Progress

## Steps

- [x] 1. Analysis of codebase complete
- [x] 2. Plan approved
- [x] 3. Create `DECISIONS.md` — full decision document
- [x] 4. Update `src/events.rs` — add `EventContractUpgraded` + emitter
- [x] 5. Update `src/lib.rs` — add `upgrade()` entry point
- [x] 6. Update `src/test_pause.rs` — add upgrade auth + storage-survival tests
- [x] 7. Update `README.md` — design-decisions section for upgradability
- [x] 8. Create `COST_MODEL.md` — per-transaction XLM cost estimate
- [x] 9. Create `DEPLOYMENT.md` — deployment guide referencing cost model
- [x] 10. Update `README.md` — reference COST_MODEL.md and DEPLOYMENT.md
- [x] 11. Implement string-length caps in `validation.rs` + new `StringTooLong` error in `types.rs`
- [x] 12. Update `COST_MODEL.md` §6 — reflect enforced caps (not just proposed)


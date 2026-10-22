I don't have the current content of `src/validation.rs` — the "Current content:" section in your message is empty. Without the existing file, I can't preserve its existing functionality (types, imports, error variants, helper functions, existing validation methods) while making the requested edits.

Could you paste the current contents of `src/validation.rs`? Once I have it, I'll return the complete updated file with:

- `validate_amount_ceiling` reading the per-anchor override from storage, falling back to the global default ceiling from instance storage, returning `AmountCeilingExceeded` on violation, and called from every `register_callback`/`batch_register_callback` validation pass.
- `Validator::validate_payload` reordered to COST_MODEL §12.5: amount bounds → four host-len checks → `asset_code` → `stellar_account`/`asset_issuer` strkey+CRC16, with the non-'G' prefix fast-reject before any base32 decode, preserving identical accept/reject outcomes.
- Doc-comments on every public item.
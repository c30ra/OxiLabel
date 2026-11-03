# Coding standards

Judgement calls for review. Mechanical rules live in the checks (`cog.toml` hooks, `.github/workflows/checks.yml`), not here.

## Database

**Check and act together**: a guard (last admin, existence, ownership) is a condition inside the write statement, and any read that explains a refused write (404 vs 409) runs in the same transaction, after the write. See `change_user` and `delete_user` in `crates/oxilabel-server/src/users.rs`.

# 14: Auth and users

**What to build:** The server requires login with a local account (argon2-hashed password, session cookie). The first user created becomes admin, and admins can create annotator accounts and reset passwords. Roles are admin and annotator.

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] On first run I am asked to create the first account, which becomes admin
- [x] Unauthenticated requests are rejected, and login and logout work with a session cookie
- [ ] Admins can create users, assign roles and reset passwords in an admin screen
- [x] Annotators cannot reach admin-only actions
- [x] API tests cover login, roles and rejected access

**Decisions (2026-10-17):** until jobs (ticket 15) narrow it, annotators may view projects, images, tiles and labels, save annotations and set image status; creating and deleting projects, editing labels, importing and removing images, exports, the host folder dialog and users are admin-only. The first account is created by whoever opens the server first (as in Gitea or Jellyfin), so first run also works under Podman, where requests do not come over loopback.

**Implementation note:** `rtag-server/src/auth.rs` holds sign-in: `GET /api/session` (whether setup is needed, and who is signed in), `POST /api/setup`, `/api/login`, `/api/logout`. Every other API route sits behind the `require_session` middleware (401 without a session); admin-only handlers take the `Admin` extractor (403 for annotators). Passwords are argon2 (PHC strings), at least 8 characters; usernames are trimmed and unique ignoring case. A session is a random 256-bit token in the `rtag_session` cookie (`HttpOnly`, `SameSite=Strict`, 30 days, not renewed on use); the database keeps only its blake3 hash. A wrong username takes as long to refuse as a wrong password. `rtag-server/src/users.rs` is the admin screen's API: list, create, and `PUT /api/users/{id}` to change a role or reset a password; a reset ends that user's other sessions, and the last admin cannot be demoted. The UI shows the sign-in (or first-account) form until signed in, an account menu (Users for admins, Log out) in the top bar of the projects and project pages, the Users screen at `/users`, and hides admin-only buttons from annotators (`/users` tells them only admins manage users). When a request gets 401 mid-use (session expired or ended by a reset), the sign-in form comes back. The session cookie has no `Secure` flag, since rtag is usually served over plain HTTP on a LAN; there is no login rate limiting. API tests sign in as an admin (`tests/common`); the browser tests create the first account through the setup form.

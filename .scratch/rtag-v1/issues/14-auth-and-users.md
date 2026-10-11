# 14: Auth and users

**What to build:** The server requires login with a local account (argon2-hashed password, session cookie). The first user created becomes admin, and admins can create annotator accounts and reset passwords. Roles are admin and annotator.

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] On first run I am asked to create the first account, which becomes admin
- [ ] Unauthenticated requests are rejected, and login and logout work with a session cookie
- [ ] Admins can create users, assign roles and reset passwords in an admin screen
- [ ] Annotators cannot reach admin-only actions
- [ ] API tests cover login, roles and rejected access

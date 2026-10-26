# 15: Autosave policy

**What to build:** Autosave is one pure state machine. Its inputs are an edit, the debounce timer firing, a save finishing (saved, refused as stale, or failed), annotations reloaded, a pre-annotation run merged, and leaving the image or the page; its outputs are the next state (saved, unsaved, saving, conflict) and whether to send a save now. The workspace keeps only the timer, sending the save, and the indicator. Leaving an image while a save is in flight waits for it and then sends the latest changes on the new base version, instead of sending a second save on the stale one whose refusal nobody sees. Reloading re-syncs the saved revision inside the policy, not at each caller.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] Save state, the saved revision and retrying live in the policy; the workspace has no autosave rules of its own
- [ ] Leaving during an in-flight save never sends a save on a stale version
- [ ] Rust tests cover every transition, including an edit during a save, leaving during a save, a conflict then reload, and a conflict then a merged run
- [ ] Browser autosave and conflict tests still pass

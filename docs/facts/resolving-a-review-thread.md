# Resolving a review thread

- GitHub answers `resolveReviewThread` with `Resource not accessible by integration` to an installation token that holds `issues` and `pull_requests` written and no `contents`. GitHub does not document it. The community discussion (https://github.com/orgs/community/discussions/44650) reports that adding Contents read and write fixed both `resolveReviewThread` and `unresolveReviewThread`.
- A refused resolve leaves a fixed thread open, and an open bot thread holds an `auto` project's merge for a ruling.
- The fix is a third grant, `Grant::Threads`, with `pull_requests` and `contents` written, minted fresh for each resolve and never kept. It can push, so kelpie's posts, a reply to a thread included, keep `Grant::Kelpie`, which still has no `contents`. The App already holds `contents: write` for workers, so its permissions are unchanged.
- The fix rests on the cited discussion and has not been checked against GitHub yet.

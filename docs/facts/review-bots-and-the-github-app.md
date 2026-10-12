# Review bots and the GitHub App

Measured 2026-10-12 on shep-pm/kelpie-scratch (public) for shep-pm/shep-kelpie#643, on the free plans of CodeRabbit and cubic and with Codex connected to the maintainer's account. The App is `kelpie-shep-pm[bot]`.

## A pull request the App opened (kelpie-scratch#17)

- CodeRabbit skipped it on open, with the status "Review skipped: bot user not eligible for review". The App's `@coderabbitai full review` comment did not start one either: the status stayed skipped.
- cubic refused the App's `@cubic-dev-ai review` within 4 seconds: "Couldn't start the review: please upgrade to a paid plan to review bot-authored PRs." It refused the maintainer's summon on the same pull request with the same words, so the limit follows the author, not the summoner.
- Codex answered the App's `@codex review` with "To use Codex here, create a Codex account and connect to github". The maintainer's `@codex review` on the same pull request drew a review 3.5 minutes later.

## A pull request the maintainer opened, summoned by the App (kelpie-scratch#18)

- cubic accepted the App's summon ("I have started the AI code review") and posted its review 39 seconds later.
- Codex refused the App the same way as on #17.
- The App adding CodeRabbit's `review please` label did not start a review. The status read "Review skipped: manual review required for this OSS repository".

## What follows

- On the free plans, a pull request authored by the App loses cubic and CodeRabbit outright, so kelpie opens pull requests as the maintainer.
- Only cubic answers a summon from the App, and Codex only answers a person linked to a Codex account. Summons stay on the maintainer's login.

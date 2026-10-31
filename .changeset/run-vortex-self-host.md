---
"@omnidotdev/run-cli": minor
---

Make `run up vortex` work out of the box. The Vortex catalog entry now enables Vortex's single-owner self-host mode (`SELF_HOST_MODE`) instead of pointing at an external Gatekeeper and Warden, so a self-hoster gets a working instance from one command with no external identity or authorization services to wire up. Built-in event schemas and workflows seed under the owner's organization.

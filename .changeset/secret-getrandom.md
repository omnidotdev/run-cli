---
"@omnidotdev/run-cli": patch
---

Generate secrets from the operating system's random source (`getrandom`) instead of the `rand` crate, removing a low-severity `rand` soundness advisory. Secret values are unchanged.

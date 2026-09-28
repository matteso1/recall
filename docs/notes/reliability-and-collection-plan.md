# Stable planning and complete event data

The owner authorized implementation while playing. Work offline; installation must wait for an
idle League phase. Preserve the running app and keep credentials/raw player data outside Git.

1. Reproduce identical-observation changes with the assembled planner. Separate conditional-model
   memory from the displayed path; retain legal purchase changes when the actual state changes.
   Compare the same held-out inputs before/after and keep a focused failing regression.
2. Add a bounded Match-v5 pilot collector using existing corpus match IDs as seeds. Fetch paired
   match/timeline responses through the official regional API, preserve raw events and stable
   identities privately, and validate ten-player coverage and matching identity sets before marking
   a pair complete. Store each response atomically and resume incomplete pairs without refetching
   valid cached match responses. Filter queue/patch before spending a timeline request.
3. Use a header-only Riot credential from an environment variable or local file, conservative
   pacing, bounded retries and Riot's Retry-After. Never log the credential or response bodies.
   Missing credentials must stop before making a request. Tests cover resumption, incomplete or
   mismatched timelines, unchanged undo events, filters, throttling and authentication failures.
4. Run the core/runtime checks and recorded-game replays, update the evidence in Git, push both
   branches, and install through the phase-aware update script only after the current game ends.

The collector does not yet fit a new model or interpret inventory transactions. Preserving exact
source events comes first; event reconstruction and player-grouped evaluation can then be built
against real paired data. A valid Riot credential is needed for the first live collection probe.

# Aggregate fixture provenance

These are raw public op.gg champion aggregate responses for deterministic, offline regression tests. Each new champion/role fixture below was fetched exactly once from the same endpoint shape used by `aggregate::load`, with region `global` and tier `emerald_plus`. The response bodies are unchanged apart from a final newline; no fields or observations were removed or combined.

The source reports patch **16.17**. Local retrieval time is in UTC. The provider's `meta.cached_at` value is preserved as received; its timezone is not inferred from the string. These observations describe players who used a build and do not establish that the build caused their wins.

| Fixture | Champion / role | Retrieved (UTC) | Endpoint |
| --- | --- | --- | --- |
| [opgg_ahri_mid.json](opgg_ahri_mid.json) | ahri (103) / mid | 2026-09-06T04:17:17.426Z | [op.gg response](https://lol-api-champion.op.gg/api/global/champions/ranked/103/mid?tier=emerald_plus) |
| [opgg_ornn_top.json](opgg_ornn_top.json) | ornn (516) / top | 2026-09-06T04:17:17.435Z | [op.gg response](https://lol-api-champion.op.gg/api/global/champions/ranked/516/top?tier=emerald_plus) |
| [opgg_darius_top.json](opgg_darius_top.json) | darius (122) / top | 2026-09-06T04:17:17.581Z | [op.gg response](https://lol-api-champion.op.gg/api/global/champions/ranked/122/top?tier=emerald_plus) |
| [opgg_lulu_support.json](opgg_lulu_support.json) | lulu (117) / support | 2026-09-06T04:17:17.687Z | [op.gg response](https://lol-api-champion.op.gg/api/global/champions/ranked/117/support?tier=emerald_plus) |
| [opgg_leesin_jungle.json](opgg_leesin_jungle.json) | leesin (64) / jungle | 2026-09-06T04:17:17.579Z | [op.gg response](https://lol-api-champion.op.gg/api/global/champions/ranked/64/jungle?tier=emerald_plus) |
| [opgg_aphelios_adc.json](opgg_aphelios_adc.json) | aphelios (523) / adc | 2026-09-06T04:17:17.705Z | [op.gg response](https://lol-api-champion.op.gg/api/global/champions/ranked/523/adc?tier=emerald_plus) |
| [opgg_udyr_jungle.json](opgg_udyr_jungle.json) | udyr (77) / jungle | 2026-09-06T04:17:17.728Z | [op.gg response](https://lol-api-champion.op.gg/api/global/champions/ranked/77/jungle?tier=emerald_plus) |

The existing `opgg_xayah_adc.json` fixture predates this acquisition. Its original population and retrieval time are not reconstructed here; its source patch and provider timestamp remain in the JSON. Its popular core sample is 882 wins / 1,542 games; the alternate order has 384 wins / 639 games. Their 95% Wilson intervals overlap, and the independent-binomial Newcombe interval for the alternate-minus-popular observed rate is about −1.66 to +7.37 percentage points.

Repeated requests on a patch share underlying matches. Fixture snapshots, cache snapshots, different regions, and overlapping tiers must not be added together to manufacture larger sample sizes. Tests preserve each response's original counts. Patch archives hold one latest snapshot per request and patch; they do not pool observations or apply an undocumented previous-patch prior.

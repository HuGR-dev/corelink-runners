# B2 DCO exception and provenance record

**Scope:** CoreLink Runners Sprint 2 bundle only
**Recorded:** 2026-09-08
**Stakeholder authorization:** gustavomalleths@gmail.com
**Reviewed tip:** dfa39eb830bc0a1b61b86667bc2f044fe546922e
**Base:** origin/main (0cc2d823)

## Decision

This is a narrow, human-authorized exception for the B2 bundle's hosted DCO execution mechanism while GitHub billing is unavailable. It records provenance and authorizes the bundle merge workflow; it does **not** declare DCO PASS, waive attribution, or sign any commit on behalf of the stakeholder. The repository's DCO checker remains unchanged and must continue to run locally against the exact merge range.

The integrated campaign contains 102 commits from origin/main..dfa39eb (101 non-merge commits plus one merge commit). Rewriting that 102-commit history to append trailers would create new commit SHAs and invalidate the reviewed SHAs, evidence bindings, manifests, and audit references already attached to this campaign. The exception preserves those immutable references and makes the residual DCO state explicit.

The inventory below is the exact 89 missing-trailer non-merge commits at the pre-tip boundary origin/main..dfa39eb^ (100 non-merge commits: 89 missing, 11 with a trailer). The reviewed tip dfa39eb itself is a later unsigned commit; therefore the unchanged checker reports 90 missing trailers over the current 101 non-merge commits. That result remains a DCO failure and is intentionally not converted to PASS by this record.

All commits created after this record, including the B2 bundle commit, require a valid Signed-off-by: trailer from the actual committer identity. No agent may add a stakeholder signoff, rewrite prior history, or describe this exception as DCO approval.

## Inventory: 89 missing-trailer commits

| # | Commit | Author | Subject |
|---:|---|---|---|
| 1 | 5d2dd56ae0501cec932d2a496920b01fe9695e23 | Gustavo Schneiter <gustavo@humangr.com> | fix(ops): enforce observability bootstrap stability window |
| 2 | 98717c4a8e7c92e510b8a46665c8132504a99b50 | Gustavo Schneiter <gustavo@humangr.com> | feat(ops): add fabricd observability key bootstrap |
| 3 | e9156184be955a47f2ee61354a336fd54d3d9b93 | Gustavo Schneiter <gustavo@humangr.com> | docs(b2): quarantine superseded one-shot evidence |
| 4 | 8120641eaf5a74537ae6dfc2d98590ed04ea74e7 | Gustavo Schneiter <gustavo@humangr.com> | test(b2): package t2-w2b cold matrix harnesses |
| 5 | 7036878c57aeafaebc35d28dc5711890d99ec068 | Gustavo Schneiter <gustavo@humangr.com> | fix(au1.8): initialize helper locals before expansion |
| 6 | 5e9dff2dd33331c4e4b8366346ab22c249605ae4 | Gustavo Schneiter <gustavo@humangr.com> | fix(au1.8): make shellcheck and set-u gates explicit |
| 7 | 651ffbd1e10f8e6495407204f6987bc92b64d404 | Gustavo Schneiter <gustavo@humangr.com> | fix(au1.8): require stable immediate fabricd rollout |
| 8 | bf31cdfdac1325a38f495302f5d6f014725d7fa8 | Gustavo Schneiter <gustavo@humangr.com> | fix(rotation): close direct live proof gaps |
| 9 | 41293ef2457b0a728fcc25faad16bd0e1506ab62 | Gustavo Schneiter <gustavo@humangr.com> | fix: use fleet internal auth header in rotation preflight |
| 10 | 551f375204569690d4e274d85af2a502ce4b35a9 | Gustavo Schneiter <gustavo@humangr.com> | ops: harden direct corelink rotation runbook |
| 11 | 2ca5c63dc95f0084f71ed7610191f816d4e774e8 | Gustavo Schneiter <gustavo@humangr.com> | docs: remove foreign project names from rotation runbook |
| 12 | 9ec9c2cfcda72576db7b24326fa842f04bb6fdc6 | Gustavo Schneiter <gustavo@humangr.com> | ops: add direct corelink secret rotation runbook |
| 13 | 38e7a502f793324af5e6b4c7a61b74f3c39a020b | Gustavo Schneiter <gustavo@humangr.com> | docs: persist crash recovery campaign checkpoint |
| 14 | 20cc115d5ffbbd89d6c5e596da53f7bbd3e81d1e | Gustavo Schneiter <gustavo@humangr.com> | docs: clarify CoreLink wire provenance |
| 15 | 2e66187a16fa31fbd3fd09afe1c74fda950e88e8 | Gustavo Schneiter <gustavo@humangr.com> | docs: make CoreLink product boundary explicit |
| 16 | bbe3980422116d4ba6b3dd1d3bba6090aeeea094 | Gustavo Schneiter <gustavo@humangr.com> | test(attestation): bound property fuzz runtime |
| 17 | 7f1724aad9a3e1f97f300d27aaec7f0a52910803 | Gustavo Schneiter <gustavo@humangr.com> | test: namespace concrete spawn fixtures |
| 18 | 84353edc0728065ab5191ef2e13657ef9f88ac7d | Gustavo Schneiter <gustavo@humangr.com> | docs: remove historical whitepaper precedence |
| 19 | b7dadb994851c41bd23df86de2b848650bdcae7d | Gustavo Schneiter <gustavo@humangr.com> | docs: supersede legacy whitepaper boundary |
| 20 | 8733c617c72ab4b250725a67ac44d6886c494848 | Gustavo Schneiter <gustavo@humangr.com> | refactor: remove legacy product wording from active surfaces |
| 21 | bd56d3f4ec42992a35e6b4ac3e42148e3e929ad7 | Gustavo Schneiter <gustavo@humangr.com> | docs: remove legacy Hugit release gates |
| 22 | 09a59da48a07171556aa6e7684eed33e5de18e39 | Gustavo Schneiter <gustavo@humangr.com> | fix: scope workspace sweep to owned names |
| 23 | 9e38419920c17b840d123e5b58b54d477e44635c | Gustavo Schneiter <gustavo@humangr.com> | fix(rotation): align bridge executable mode policy |
| 24 | 527f32ac38be1c678e63aee048f7d76d2a28f429 | Gustavo Schneiter <gustavo@humangr.com> | fix(rotation): accept repository executable controller mode |
| 25 | 42a3e3c199ed5403eb2eea2ab32df20c76803ab7 | Gustavo Schneiter <gustavo@humangr.com> | fix: harden namespace ownership and legacy cleanup |
| 26 | f21e8bb8e8dc050f7b1027179b5089dbe8a986ad | Gustavo Schneiter <gustavo@humangr.com> | refactor: decouple active contract ownership wording |
| 27 | beb49378e7b5de7f98238c0b9741df2fe4aafe31 | Gustavo Schneiter <gustavo@humangr.com> | feat(rotation): package Corelink B2 rotation gates |
| 28 | 76fac49b06203bbe1a6b88fdac2ed3e88ef91571 | Gustavo Schneiter <gustavo@humangr.com> | docs: close legacy integration cold-review gaps |
| 29 | 820576823b970e307d2daf9ca4093e1ac19877dc | Gustavo Schneiter <gustavo@humangr.com> | fix: harden AU1.8 promotion gates |
| 30 | d3607804e2c1080c1de6bdb63029ac6e36b6cafb | Gustavo Schneiter <gustavo@humangr.com> | refactor: decouple runtime namespaces from legacy apps |
| 31 | 1143e4353c5b8594943109bda76371682ac2d124 | Gustavo Schneiter <gustavo@humangr.com> | docs: decouple CoreLink promotion from Hugit |
| 32 | 1796b6a9afd86ece35e853cced3ad9c580dca802 | Gustavo Schneiter <gustavo@humangr.com> | chore: decouple runtime config from discontinued products |
| 33 | 007667afdb620e5db245c6b3147c85e12b1d68af | Gustavo Schneiter <gustavo@humangr.com> | docs: refresh B2 compaction checkpoint |
| 34 | 27c08dd76650bc3385c69ce60f7fea5726d05a82 | Gustavo Schneiter <gustavo@humangr.com> | fix(fabricd): preserve completion webhooks during admission freeze |
| 35 | 99d5e39e66a69d44226dc9eed0faf0f7b384156f | Gustavo Schneiter <gustavo@humangr.com> | feat(spawn): extend admission freeze across provider paths |
| 36 | 8571f3cf38f11cca9b1f0752bf028e3dd982bb52 | Gustavo Schneiter <gustavo@humangr.com> | feat(fabricd): add edge admission freeze switch |
| 37 | bcd7920d782196bb8f8c29ee294dce5c19dfc883 | Gustavo Schneiter <gustavo@humangr.com> | docs: record T2-W2b recovery state |
| 38 | e88ded3ec7c5e74c03f628908b14208da9a90710 | Gustavo Schneiter <gustavo@humangr.com> | fix: use canonical evidence repository name |
| 39 | 1729a8891461620fb64e7aea1521c170780d667d | Gustavo Schneiter <gustavo@humangr.com> | docs: record final spawn restoration |
| 40 | 70676d3895d78dade4227ed19064858d43197e3f | Gustavo Schneiter <gustavo@humangr.com> | docs: record T2-W2b incident recovery |
| 41 | 4e2aff7a30b100bb06d24a79609d4644db38023d | Gustavo Schneiter <gustavo@humangr.com> | docs: retain T2-W2b cold-start gate |
| 42 | 41600c9eb7f0d6699a2f9c1c6530ccba6c834633 | Gustavo Schneiter <gustavo@humangr.com> | docs: state fabricd in-memory sleep limitation |
| 43 | 672e05cc46550fe48e51ba0d8adcbd4be434ef00 | Gustavo Schneiter <gustavo@humangr.com> | docs: document conditional fabricd keepalive and scale-to-zero |
| 44 | fa13e8a305b13f1dcbfa4cf0ebd15f83f1f1da52 | Gustavo Schneiter <gustavo@humangr.com> | docs: reconcile final T2-W2b rollback evidence |
| 45 | 48052654943cffaf6827a2a7a729012d2aa2df5b | Gustavo Schneiter <gustavo@humangr.com> | docs: record A2.7 local gate recovery |
| 46 | 9273080e0485a202bdca5f1c2899e5169fb48e45 | Gustavo Schneiter <gustavo@humangr.com> | test(worker): preserve ttl job aliases after teardown |
| 47 | 9463dc87620424142bfb62ed9a72cfbbbba40d0b | Gustavo Schneiter <gustavo@humangr.com> | fix(worker): restore bounded admission budget authority |
| 48 | 0b7633f8b875c79b452a54b9f896fbc39cf8413b | Gustavo Schneiter <gustavo@humangr.com> | test: align reconciler fixtures with atomic spawn claims |
| 49 | 98dc349174812408432ba4c5897f65d5ee58df00 | Gustavo Schneiter <gustavo@humangr.com> | test(worker): model jit registration cleanup in warm retry |
| 50 | 9006b74d8ad2ac1309cd3ea4064443d7da6b2d04 | Gustavo Schneiter <gustavo@humangr.com> | docs: reconcile T2-W2b canonical evidence |
| 51 | 5cede249c578315621d20a3ba85729c54d5c04e8 | Gustavo Schneiter <gustavo@humangr.com> | docs: record T2-W2b retry rollout |
| 52 | 9fb7cc4890ee83248caf2694bfb5d39f4e45c7d8 | Gustavo Schneiter <gustavo@humangr.com> | docs: record T2-W2b rollback recovery |
| 53 | 15d85a5dd703f4a64101b7de64a2e5b8a3ba7815 | Gustavo Schneiter <gustavo@humangr.com> | config: pin T2-W4 devenv image |
| 54 | cb9f417a2b00b3aee6c0b476ec5d966a1cac5a33 | Gustavo Schneiter <gustavo@humangr.com> | docs: record T2-W4 final source security |
| 55 | defff94e401bc7dca4b70eeb709de3b2a4e76627 | Gustavo Schneiter <gustavo@humangr.com> | fix(devenv): scan real PEM blocks only |
| 56 | 17abde770500044d9bdeb9c9999c1da5c20e3c6e | Gustavo Schneiter <gustavo@humangr.com> | docs: record T2-W4 security approval |
| 57 | 377dbb0e38f58be467964ecfdae6b224d0fdcd90 | Gustavo Schneiter <gustavo@humangr.com> | fix(devenv): handle private-key scanner errors explicitly |
| 58 | 4a426e283a1c1627304c13a560a52b690661bad1 | Gustavo Schneiter <gustavo@humangr.com> | fix(devenv): remove code-server private key fixture |
| 59 | d726e390a75a8c2b94ba8bbf407c38a9a7c09575 | Gustavo Schneiter <gustavo@humangr.com> | docs: record T2-W4 local image verdict |
| 60 | c30184c36d6c6e8c5323596bd0c0cbbbb40fa156 | Gustavo Schneiter <gustavo@humangr.com> | fix(devenv): normalize coder identity on Ubuntu base |
| 61 | 31eab27f94c97d7cfd63759b15e2b1d3e0744ef8 | Gustavo Schneiter <gustavo@humangr.com> | fix(devenv): pin builder bases and require provenance |
| 62 | 90e1a1d864aebcb62915fed56e2ebc9e0fd39d03 | Gustavo Schneiter <gustavo@humangr.com> | docs(devenv): correct runtime base comment |
| 63 | 3d5c53caf1fa00ef454ec3b630a16eb65b035f67 | Gustavo Schneiter <gustavo@humangr.com> | fix(devenv): ship complete code-server runtime |
| 64 | ccdf2967a5996c7dc33697b086f19ff03b283f47 | Gustavo Schneiter <gustavo@humangr.com> | build: refresh devenv toolchain and artifact pins |
| 65 | 02e152ee86fa0b315776f79e9454d267adecaf8d | Gustavo Schneiter <gustavo@humangr.com> | chore(fabricd): reconcile live image pin and capacity docs |
| 66 | 8140abae9c7540bc35359dc12b9fc5c769f8b95c | Gustavo Schneiter <gustavo@humangr.com> | docs: record T2-W6 local runbook |
| 67 | ab52c15e7a9e4b75d936c2ea0a26942521af9bb4 | Gustavo Schneiter <gustavo@humangr.com> | docs: record T2-W2b local preflight |
| 68 | f34dd580b39d73ab5ea4f4babaac6c6031762e62 | Gustavo Schneiter <gustavo@humangr.com> | docs: record T3-W9 source acceptance |
| 69 | a0ae7468405685077e9b0072dec0f2eee89c9690 | Gustavo Schneiter <gustavo@humangr.com> | fix(cloudflare): require runner deletion confirmation |
| 70 | 20aaad75b89e541d944d5ed7624390042075588c | Gustavo Schneiter <gustavo@humangr.com> | fix(cloudflare): retire confirmed retry attempts |
| 71 | 440a4e3e33974126522f87ebcb710fbf286a9cee | Gustavo Schneiter <gustavo@humangr.com> | test(cloudflare): bind containment recovery witness |
| 72 | 1904747a46ef64958d6d8cf9c7772b2bd8e714cd | Gustavo Schneiter <gustavo@humangr.com> | test(cloudflare): cover installation tombstone recovery fences |
| 73 | b44e25037c4d93f99a85df463d54abd9d435dc41 | Gustavo Schneiter <gustavo@humangr.com> | fix(cloudflare): fail closed on malformed installation allowlists |
| 74 | 73675d35c4ef9d80d69a8ef06f4dd564316d79e1 | Gustavo Schneiter <gustavo@humangr.com> | fix(cloudflare): checkpoint exact teardown before release |
| 75 | 9693d52bae1fff44362ef92d3bb116225f939922 | Gustavo Schneiter <gustavo@humangr.com> | test(cloudflare): cover post-start cleanup fault matrix |
| 76 | 0541333451a124400f0b6a8679006b27317bd381 | Gustavo Schneiter <gustavo@humangr.com> | fix(cloudflare): fence tombstoned retry effects |
| 77 | 4d1976def7c1715e6d1b8d912d5fbd7b314bf7c4 | Gustavo Schneiter <gustavo@humangr.com> | fix(cloudflare): durably fence post-start cleanup and installation deletion |
| 78 | 388ae7927ff6697b9a6d913414327740314c619d | Gustavo Schneiter <gustavo@humangr.com> | docs: record T8-W3 and T3-W14 acceptance |
| 79 | 29b831d0a8373254ed992421e8f295ba508a0403 | Gustavo Schneiter <gustavo@humangr.com> | fix(worker): fence completion claims by provider identity |
| 80 | 7477ed9fd67e0fa2562b3d8cb6b77b23ebe497bd | Gustavo Schneiter <gustavo@humangr.com> | feat(worker): atomically fence spawn claims |
| 81 | a6bab00ca0b0b3d129aa3a248f5c0399e04740df | Gustavo Schneiter <gustavo@humangr.com> | docs: record T3-W2 acceptance and T8-W1 readiness |
| 82 | 22a3d1e21892f78bfdceb9243fbb924c0a824a3f | Gustavo Schneiter <gustavo@humangr.com> | test(cloudflare): pin teardown failure retention |
| 83 | dd41b397692c4d0608a333937922b7ff2f24971e | Gustavo Schneiter <gustavo@humangr.com> | docs: enable dispatch from B2 post gate |
| 84 | 220090f8d9a0408c955d2f7a915308f6496028f8 | Gustavo Schneiter <gustavo@humangr.com> | docs: promote B2 dispatch registry freeze |
| 85 | fdf3a27fab6aa2231323ed6a95af369ae46f136a | Gustavo Schneiter <gustavo@humangr.com> | docs: preserve B2 source provenance |
| 86 | 6eb3f27c293ef1ec33e1b72704798f8f4606213e | Gustavo Schneiter <gustavo@humangr.com> | docs: normalize B2 ledger freeze records |
| 87 | 2a74158dd738a45427987cf6170390a535c04a1e | Gustavo Schneiter <gustavo@humangr.com> | docs: clarify B2 F007 closure and source evidence |
| 88 | 265557bf018dc29f7cdf2cd0c0e867a5a95fc9a7 | Gustavo Schneiter <gustavo@humangr.com> | docs: reconcile B2 source and external gates |
| 89 | 1eff54a500e4cbb9b1df42b8ff18d70a6f5b6771 | Gustavo Schneiter <gustavo@humangr.com> | docs: freeze B2 scaffold on merged B1 baseline |

## Promotion boundary

The B2 bundle may use the stakeholder-authorized local Docker CI substitution already recorded in [WAIVERS.md](../WAIVERS.md), then undergo the documented objective review and merge gates. Hosted GitHub Actions execution remains pending billing recovery. The DCO outcome is carried as **EXCEPTION / NOT PASS** until the 102-commit campaign is superseded by a separately reviewed history or the owner explicitly resolves the repository policy through an approved process.


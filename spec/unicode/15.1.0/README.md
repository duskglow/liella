# Unicode 15.1.0 inputs

Normative inputs for the generated Unicode tables (implementation-plan.md IP-19, D-X.4). The
spec pins the Unicode version glibc 2.39 uses (§5: "Unicode 15.1.0 in 2.39"). These files are
unmodified copies, distributed under the Unicode License v3 (`license.txt`, compatible with
Liella's LGPL-2.1-or-later terms; its notice must accompany them and the tables generated from
them).

Fetched 2026-10-01 (owner approval) from:

* `https://www.unicode.org/Public/15.1.0/ucd/UCD.zip`
  (SHA-256 `cb1c663d053926500cd501229736045752713a066bd75802098598b7a7056177`): every file
  here except the ones below; `emoji-data.txt` is `emoji/emoji-data.txt` in the archive.
* `https://www.unicode.org/Public/UCA/15.1.0/allkeys.txt`: the Default Unicode Collation
  Element Table (DUCET), the base of the ISO 14651 collation engine.
* `https://www.unicode.org/license.txt`.
* `https://www.unicode.org/Public/idna/15.1.0/IdnaMappingTable.txt` and
  `https://www.unicode.org/Public/15.1.0/ucd/ArabicShaping.txt` (fetched 2026-10-03, phase 10
  part 4): the UTS #46 mapping and the joining types, for the built-in IDNA (spec §16.2, D-16.4).

| File | SHA-256 | Used for |
|---|---|---|
| `UnicodeData.txt` | `2fc713e6a31a87c4850a37fe2caffa4218180fadb5de86b43a143ddb4581fb86` | categories, simple case mappings, decompositions |
| `DerivedCoreProperties.txt` | `f55d0db69123431a7317868725b1fcbf1eab6b265d756d1bd7f0f6d9f9ee108b` | Alphabetic, Lowercase, Uppercase, Default_Ignorable |
| `PropList.txt` | `05672956317b6296bc2ec3d6cef1f6452b57ff4f2efc6dc55b0a19277d5fcfd1` | White_Space and other binary properties |
| `EastAsianWidth.txt` | `b08191401dc125f4e84ef262a95754faae6b737c79538e17ea9664a63434e94e` | display width (§6.5) |
| `emoji-data.txt` | `d7aef489c8fe4c14f09ea5695200277c6b93ac82ac60845cdd2161b0d6835cc1` | emoji presentation width |
| `HangulSyllableType.txt` | `8539c7fdb3e8cd9efcc04a420061fa56a74762a3cb60fc9962c26093c8944427` | conjoining jamo width |
| `CaseFolding.txt` | `4e55acfdc32825a22e87670e9056a3bf94ad7c5400065778e9e10f8314372bcf` | case-insensitive comparison |
| `SpecialCasing.txt` | `55a477efd933a52cd27e6a9bf70265bb2d8814af31aab07767abc8eb421f27ef` | full case mapping (internal, D-5.2) |
| `CompositionExclusions.txt` | `59d2d9e3dfdf0a999cf9dae11d594f053631222679a2f5710315ea07f7fe82af` | normalization |
| `DerivedNormalizationProps.txt` | `8875dccee2bc1a7c1fe568a3b502a9e78c9e0495afd96b6568b4294d0ed1f7e1` | normalization quick checks |
| `NormalizationTest.txt` | `871238e37e3be0696ec2bd0891119a041b052da1a84485eda05a5438724b223e` | test data |
| `ReadMe.txt` | `0d2da782ead4e85630d510f50808355e8c3355e670841d257dd1e6fbd40db9fa` | the archive's version statement |
| `allkeys.txt` | `0c640f7c7e9f91b7b1b4f904c142802e086bd5c1324659ba2aa6c9d6d5c3e16d` | collation (DUCET) |
| `license.txt` | `e7a93b009565cfce55919a381437ac4db883e9da2126fa28b91d12732bc53d96` | Unicode License v3 |
| `IdnaMappingTable.txt` | `402cbd285f1f952fcd0834b63541d54f69d3d8f1b8f8599bf71a1a14935f82c4` | UTS #46 mapping and validity (IDNA, §16.2) |
| `ArabicShaping.txt` | `c289c5fbc632fe3d5f969b5592d5f7fbdbc4b01f0f1370a38b83df5f6b94310c` | joining types (IDNA CONTEXTJ, RFC 5892 A.1) |

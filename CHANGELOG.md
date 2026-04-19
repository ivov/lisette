# Changelog

Lisette is under active development. Any version before 1.0.0 may include breaking changes.

## [0.1.13](https://github.com/ivov/lisette/compare/lisette-v0.1.12...lisette-v0.1.13) - 2026-04-18

- fix: don't wrap named function type returns in Option [#103](https://github.com/ivov/lisette/pull/103) [`0e31195`](https://github.com/ivov/lisette/commit/0e311953357d3a7df4dac182dee7693e2bc7caa7)
- fix: type os.Exit and log.Fatal* as Never [#120](https://github.com/ivov/lisette/pull/120) [`7dd3e71`](https://github.com/ivov/lisette/commit/7dd3e71960ca51d1477f269d7b17e388f862f9b8)
- fix: value enum match arms and interface-slotted tuple returns [#101](https://github.com/ivov/lisette/pull/101) [`2791a57`](https://github.com/ivov/lisette/commit/2791a5720d8b42dc79d736f9e7d90925ce8304c7)
- fix: emit break after switch in guarded match [#104](https://github.com/ivov/lisette/pull/104) [`1644ca0`](https://github.com/ivov/lisette/commit/1644ca04959b517743c299a025a0619d5cfa8a4f)
- fix: use declared package name over path segment as Go qualifier [#106](https://github.com/ivov/lisette/pull/106) [`09a76f0`](https://github.com/ivov/lisette/commit/09a76f0be5ca203f98b3bbfe94dccdf6c7afc234)
- fix: emit value equality for Go sentinel patterns like Err(io.EOF) [#108](https://github.com/ivov/lisette/pull/108) [`bf3d0da`](https://github.com/ivov/lisette/commit/bf3d0dab4bd990c80b305453df2212edc51c4cb6)
- fix: preserve Go interface and alias types in match-arm tuple slots [`d73fdbe`](https://github.com/ivov/lisette/commit/d73fdbe6ae6fe076a6a8fe94799443f0d98a5800)
- fix: classify nullable Go function aliases as NullableReturn [`d2fd7f6`](https://github.com/ivov/lisette/commit/d2fd7f61022c66aad3cbf755849162c61d96bd1f)
- fix: don't flag wildcard as redundant in interface match [#121](https://github.com/ivov/lisette/pull/121) [`86dcafd`](https://github.com/ivov/lisette/commit/86dcafd04ab5217137ea5d888a919f46da30743c)
- fix: treat .d.lis types as public in register_module [`95d5704`](https://github.com/ivov/lisette/commit/95d5704fefecc4c68dfff9bb42f835dc20004cf3)
- fix: allow closures returning concrete types as Go function aliases [`96b5a31`](https://github.com/ivov/lisette/commit/96b5a3122c85c542176ef7303c63ea70901145cf)
- fix: formatter moves comments into impl, try, and recover blocks [#115](https://github.com/ivov/lisette/pull/115) [`b7d1f3a`](https://github.com/ivov/lisette/commit/b7d1f3a9508f1fd75eac2cdeaaf151ab6efc08dd)
- fix: preserve type alias names in emitter output [#122](https://github.com/ivov/lisette/pull/122) [`49a0817`](https://github.com/ivov/lisette/commit/49a081719deeb679dd247b68a984c218bb92705b)
- feat: add byte_at and rune_at to string [#123](https://github.com/ivov/lisette/pull/123) [`c2188a3`](https://github.com/ivov/lisette/commit/c2188a3aa29f7f15595c50d7aedf35a1f152a2e3)
- fix: desugar pipeline operator inside slice literals [`198bfaa`](https://github.com/ivov/lisette/commit/198bfaacfd2b644ad7785fcecc9488669c673e58)

## [0.1.12](https://github.com/ivov/lisette/compare/lisette-v0.1.11...lisette-v0.1.12) - 2026-04-15

- refactor: extract shared go output + finalize helpers [#91](https://github.com/ivov/lisette/pull/91) [`b4ceb49`](https://github.com/ivov/lisette/commit/b4ceb49c7914a926590fd2fd5b505f55e5238c02)
- fix: regenerate missing Go typedefs before check/build/run [#88](https://github.com/ivov/lisette/pull/88) [`cc6912b`](https://github.com/ivov/lisette/commit/cc6912be7cd3ef069468d1b668c81d72dff58bcb)
- fix: emit named empty Go interfaces as Lisette interfaces [#86](https://github.com/ivov/lisette/pull/86) [`029bb6e`](https://github.com/ivov/lisette/commit/029bb6e55888f0ac59acc9293250e1a88f4ee9b8)
- fix: synthesize Go interface adapters for Lisette impls [#92](https://github.com/ivov/lisette/pull/92) [`ccea037`](https://github.com/ivov/lisette/commit/ccea03769210d8995102686e58065710f7318d41)
- fix: emit missing imports for enum variant payload types [#83](https://github.com/ivov/lisette/pull/83) [`c663661`](https://github.com/ivov/lisette/commit/c6636612d7da8f5933f222484655bebc80750251)

## [0.1.11](https://github.com/ivov/lisette/compare/lisette-v0.1.10...lisette-v0.1.11) - 2026-04-14

- fix: only translate invalid version errors pinned to user target [`76c0037`](https://github.com/ivov/lisette/commit/76c0037a584f22b1d9835e935d5a61b37397ec4d)
- fix: reject unparseable bindgen output before caching in lis add [`e6fbc1f`](https://github.com/ivov/lisette/commit/e6fbc1f34a7314561980f7007eae8e946d8c5ade)
- fix: prune stale .go files from target on rebuild [#82](https://github.com/ivov/lisette/pull/82) [`25c1d58`](https://github.com/ivov/lisette/commit/25c1d5807f04c032d24376e400208dcebe3d01dd)
- fix: stop prefixing commit hashes with v in lis add [`47196ec`](https://github.com/ivov/lisette/commit/47196ecf0bd4fb4a1bc932f2eae6b9b6db53d2d6)
- fix: report missing repo segment in github.com module path [`00f297c`](https://github.com/ivov/lisette/commit/00f297c7f9bdf7b5738375705cb9bdce6c29fdab)
- fix: distinguish package-local Option/Result/Partial from prelude [`3342aa8`](https://github.com/ivov/lisette/commit/3342aa8360830474a035f70a58c3cb071cb6cccb)
- fix: preserve snake_case field name on ref receiver access [#80](https://github.com/ivov/lisette/pull/80) [`1fa6205`](https://github.com/ivov/lisette/commit/1fa6205d26c5ea94996091660f90caca6eb39842)

## [0.1.10](https://github.com/ivov/lisette/compare/lisette-v0.1.9...lisette-v0.1.10) - 2026-04-14

- docs: mention goland in homepage [`834c1d3`](https://github.com/ivov/lisette/commit/834c1d31e734012da93f77af18f851376ce12b39)
- feat: goland support [#76](https://github.com/ivov/lisette/pull/76) [`59ef661`](https://github.com/ivov/lisette/commit/59ef6616272b29483f5ef5edfd6edac159c1176d)
- fix: default Go import alias to declared package name [#72](https://github.com/ivov/lisette/pull/72) [`af71eca`](https://github.com/ivov/lisette/commit/af71ecacc5fbe85ec02851aa12244be3202f6b59)
- fix: accept \a \b \f \v escape sequences in string and rune literals [#73](https://github.com/ivov/lisette/pull/73) [`7b7d7ce`](https://github.com/ivov/lisette/commit/7b7d7ce4d8bd8b8d5ae1c8dc828fcc4a5377dee5)

## [0.1.9](https://github.com/ivov/lisette/compare/lisette-v0.1.8...lisette-v0.1.9) - 2026-04-13

- fix: harden lis add command [#64](https://github.com/ivov/lisette/pull/64) [`f8df4fb`](https://github.com/ivov/lisette/commit/f8df4fb9a35c01d5ec4f00d8345cfa0bde464a50)
- fix: integer literal edge cases and unicode escape validation [`3b7a2b9`](https://github.com/ivov/lisette/commit/3b7a2b9ca650984bf2547ebc8c24a72f51a7abd5)
- fix: reject static method called on an instance [#69](https://github.com/ivov/lisette/pull/69) [`efacd5f`](https://github.com/ivov/lisette/commit/efacd5f42a9f349806c7fd2c8096abe017ebebe7)
- fix: erase self-referential bounds on interface type parameters [#68](https://github.com/ivov/lisette/pull/68) [`a92f8df`](https://github.com/ivov/lisette/commit/a92f8df96afa360f1b5fb3ee3450b30f44d94379)
- fix: allow type alias to fn as type conversion [#65](https://github.com/ivov/lisette/pull/65) [`b806427`](https://github.com/ivov/lisette/commit/b806427096288fc2b39051eae4aaa7e518c06298)

## [0.1.8](https://github.com/ivov/lisette/compare/lisette-v0.1.7...lisette-v0.1.8) - 2026-04-12

- chore: render changelog as flat list of all commits [`08d6a72`](https://github.com/ivov/lisette/commit/08d6a72e6f83d97b1a9e531b639554432f7eefde)
- feat: groundwork for lis add command [#55](https://github.com/ivov/lisette/pull/55) [`e4a15e7`](https://github.com/ivov/lisette/commit/e4a15e7a4937ad498d21f67a20b0e86f1e717596)
- fix: reject relative-path imports with clear diagnostic [#58](https://github.com/ivov/lisette/pull/58) [`21389f0`](https://github.com/ivov/lisette/commit/21389f0264e60da9d7dcf8eb6d8398bd2c82c810)
- fix: register impl blocks after sibling-file type definitions [#57](https://github.com/ivov/lisette/pull/57) [`85a0d5f`](https://github.com/ivov/lisette/commit/85a0d5fe72f1c226fe8a59eacb33c2d7a9667359)
- refactor: reorganize deps crate [`09beac3`](https://github.com/ivov/lisette/commit/09beac374f09f4766d67598a203d41eabf8a70bd)
- refactor: simplify bindgen invocation [`262cc20`](https://github.com/ivov/lisette/commit/262cc20c20cad53d61415b0538f4cf9be7a65dc2)
- refactor: simplify typedef resolver [#50](https://github.com/ivov/lisette/pull/50) [`07a7a45`](https://github.com/ivov/lisette/commit/07a7a453b2deeef6660a5e2f56f66801af3012bc)

## [0.1.7](https://github.com/ivov/lisette/compare/lisette-v0.1.6...lisette-v0.1.7) - 2026-04-11

- chore: include license file in published crates [#48](https://github.com/ivov/lisette/pull/48) [`e7a6205`](https://github.com/ivov/lisette/commit/e7a62053f6f34f41a68a679286cab1f63fcfbbf7)
- feat: publish bindgen as a Go module [#47](https://github.com/ivov/lisette/pull/47) [`0c2b480`](https://github.com/ivov/lisette/commit/0c2b4800bad2933c9832106963dc77c629c39138)
- feat: compiler awareness of third-party Go deps [#44](https://github.com/ivov/lisette/pull/44) [`88ff1a6`](https://github.com/ivov/lisette/commit/88ff1a6acf3d535eda6b21f178861a0bb51160dd)
- fix: validate type parameter bounds on type definitions [#43](https://github.com/ivov/lisette/pull/43) [`0191647`](https://github.com/ivov/lisette/commit/0191647e20a5f19d3b0b2782992b8f56ee3d5a23)
- fix: use program::Visibility in fuzz infer target [`41ca5bb`](https://github.com/ivov/lisette/commit/41ca5bb2b796ddd30bd9f46b475da034dc1e3ee2)
- chore: update fuzz lockfile versions to v0.1.6 [`1423dca`](https://github.com/ivov/lisette/commit/1423dcab49b4ee15ad9b8b82177f38d8243984d3)
- fix: resolve Forall gracefully and add registration to fuzz target [`480ca6e`](https://github.com/ivov/lisette/commit/480ca6e32810d9e6b002a387a14face4934cd8c2)

## [0.1.6](https://github.com/ivov/lisette/compare/lisette-v0.1.5...lisette-v0.1.6) - 2026-04-09

- fix: deduplicate diagnostics for const type annotations [`09f7d2c`](https://github.com/ivov/lisette/commit/09f7d2c536f21be76bd4cd5ec62783ce966f5d5b)
- fix: deduplicate diagnostics for function signature annotations [`a5f70a7`](https://github.com/ivov/lisette/commit/a5f70a74302b335586a28715c7ac2f9f5980fd6c)
- fix: minor cli adjustments [#40](https://github.com/ivov/lisette/pull/40) [`ef2d431`](https://github.com/ivov/lisette/commit/ef2d4311d62b1195e67f6ba34b23ad6ddd033902)
- docs: add favicon [`f5ef52c`](https://github.com/ivov/lisette/commit/f5ef52c7ef7c6438ddfbafb33e7123d33cadff62)
- feat: add `completions` CLI command [#39](https://github.com/ivov/lisette/pull/39) [`907b630`](https://github.com/ivov/lisette/commit/907b6304d904e306a88118a9d951a3c76e0e5fa2)
- chore: exclude stdlib typedef bumps from changelog [`a55f520`](https://github.com/ivov/lisette/commit/a55f52030c35ec9e390d08e6324b6f98e9e59a14)
- fix: resolve non-generic type aliases as qualifiers cross-module [#37](https://github.com/ivov/lisette/pull/37) [`1a4c743`](https://github.com/ivov/lisette/commit/1a4c7439fe1144fb08caf40d47bf7ee9a1df4d6d)
- ci: guard release comment calls against transient failures [`2b90fe0`](https://github.com/ivov/lisette/commit/2b90fe0ef3a37c606b85ab3bc8a712c5c348906d)
- ci: add issues write permission for release comments [`f4ed6b1`](https://github.com/ivov/lisette/commit/f4ed6b13484ff66be49eda84d46316ea9f0162e6)
- chore: auto-commit stdlib typedefs in regeneration recipe [`aebc6a2`](https://github.com/ivov/lisette/commit/aebc6a26edc7bfde3283b3a0ef55f2c37bb810b7)

## [0.1.5](https://github.com/ivov/lisette/compare/lisette-v0.1.4...lisette-v0.1.5) - 2026-04-08

- ci: comment on closed issues in release workflow [`73143dc`](https://github.com/ivov/lisette/commit/73143dc4bb5a3936da2da82e25ec72b435250dd7)
- fix: skip pattern analysis on import cycle [#34](https://github.com/ivov/lisette/pull/34) [`88eb7fa`](https://github.com/ivov/lisette/commit/88eb7fae5cf4ef71ee205722d16e7ab7c4d0039b)
- feat: add playground to docs site [#27](https://github.com/ivov/lisette/pull/27) [`d917711`](https://github.com/ivov/lisette/commit/d917711bd556bd6e8e747e4000ec2454686d42a7)
- fix: interface subtype satisfaction through type variables [#31](https://github.com/ivov/lisette/pull/31) [`020c407`](https://github.com/ivov/lisette/commit/020c407a88e5556151afe173286fada1f26a1b8b)
- ci: skip check job on release-plz commits [`2497b62`](https://github.com/ivov/lisette/commit/2497b62e6531a9a201f1facc2df8e90c997ee3a4)

## [0.1.4](https://github.com/ivov/lisette/compare/lisette-v0.1.3...lisette-v0.1.4) - 2026-04-07

- ci: comment on PRs included in a release [`04b5a82`](https://github.com/ivov/lisette/commit/04b5a8273a75079c500abb9d1fd15157413a043d)
- feat(editors): add info for helix [#21](https://github.com/ivov/lisette/pull/21) [`7f9cd3c`](https://github.com/ivov/lisette/commit/7f9cd3c9ea2f6d2007f825e27e762d72a311d325)
- fix: skip auto-generated stringer on user string + goString [`cc45b35`](https://github.com/ivov/lisette/commit/cc45b35af73496476e5ed77e6b9f0809f962ccdb)
- fix: swap string method for go string method [#17](https://github.com/ivov/lisette/pull/17) [`891cf8d`](https://github.com/ivov/lisette/commit/891cf8d9c49d98b3c12156858fd6579a9e75fffc)
- fix: ice when calling generic type as function [#28](https://github.com/ivov/lisette/pull/28) [`02ec377`](https://github.com/ivov/lisette/commit/02ec377932aea690e949257f171a4e6b014dc15a)
- fix: support octal escape sequences [#22](https://github.com/ivov/lisette/pull/22) [`a9a5872`](https://github.com/ivov/lisette/commit/a9a5872374f9d582c2cd18c5585b95b2e2d02188)
- fix: add typo suggestions for CLI subcommands [#23](https://github.com/ivov/lisette/pull/23) [`befe96a`](https://github.com/ivov/lisette/commit/befe96aa284c41b3d55f2b18e22525980eaa24f4)
- docs: update Zed extension PR link [`2f76686`](https://github.com/ivov/lisette/commit/2f76686f3bd4d54ca99303a8d5e20a3f1609e354)

## [0.1.3](https://github.com/ivov/lisette/compare/lisette-v0.1.2...lisette-v0.1.3) - 2026-04-06

- chore: add pre-1.0 breaking changes policy [`9ccebaa`](https://github.com/ivov/lisette/commit/9ccebaa7a495beb8f5aaa7c739a51850981ef0c6)
- fix: add Partial<T, E> for non-exclusive (T, error) returns [#18](https://github.com/ivov/lisette/pull/18) [`9887612`](https://github.com/ivov/lisette/commit/98876122ecc5c7c4b72417233005c6088c6102c4)
- docs: note Zed extension is pending review [`f2cdfa3`](https://github.com/ivov/lisette/commit/f2cdfa3cc20224d8389668087d523ac21953e90f)
- fix: make prelude variant name registration collision-safe [`0cc21aa`](https://github.com/ivov/lisette/commit/0cc21aa98fdf2256ef10f168d4f09ed6e6cb6565)
- refactor: replace DiscardedTailFact boolean with enum [`d7e9103`](https://github.com/ivov/lisette/commit/d7e91033ae2be001b029b2f310eb25af6d395243)
- chore: remove .cargo from gitignore [`a1836f4`](https://github.com/ivov/lisette/commit/a1836f40d5ada27db809193801ce5dddfdba92e7)
- chore: remove stale comment [`6e17d01`](https://github.com/ivov/lisette/commit/6e17d015aec8d6592538dff3235f75fd09137e0c)
- fix: decouple diagnostic coloring from environment [#6](https://github.com/ivov/lisette/pull/6) [`b5164b3`](https://github.com/ivov/lisette/commit/b5164b398265a567605b5a7311248886d347dc74)
- ci: add cargo-deny for dependency auditing [`a377264`](https://github.com/ivov/lisette/commit/a3772645d29f74173d2559134db5ad4491946fd0)
- ci: pin Rust toolchain via rust-toolchain.toml [`fb092e5`](https://github.com/ivov/lisette/commit/fb092e5fe245ce3efc7e09da393127c23ceffefb)
- fix: guard against stack overflow from chained postfix operators [`7d66c55`](https://github.com/ivov/lisette/commit/7d66c555ebe1ac6029f760c5adee063cac9c81cf)
- chore: clean up changelog and release-plz config [`d8cd590`](https://github.com/ivov/lisette/commit/d8cd590c2390a65dc68a552c9ba4be9cfc917cea)
- chore: match nested files in lefthook format check glob [`b1afdcc`](https://github.com/ivov/lisette/commit/b1afdccee07aede687060517b7206527c58aa163)
- feat: add version override to bindgen stdlib command [`5e2cba4`](https://github.com/ivov/lisette/commit/5e2cba43fd3d76fa46508777618ea12c85ece83f)
- fix: wrap interface globals in Option when not provably non-nil [`2703398`](https://github.com/ivov/lisette/commit/270339884ed000af61225a2af297c6d3ce951025)
- chore: regenerate stdlib typedefs [`b7324fb`](https://github.com/ivov/lisette/commit/b7324fb8bea0f1c9cd8feb642c6bff021569450d)
- fix: detect typed nils in Go interface wrapping [`7325047`](https://github.com/ivov/lisette/commit/73250472dbf48e4d527ba5f499794717e0759ed3)

## [0.1.2](https://github.com/ivov/lisette/compare/lisette-v0.1.1...lisette-v0.1.2) - 2026-03-31

- fix: fold Range sub-expressions in AstFolder [`2d357f1`](https://github.com/ivov/lisette/commit/2d357f179f8f4536b5bc723fad55b438dc2113cf)
- fix: prevent OOM by lowering max parser errors to 50 [`c123f33`](https://github.com/ivov/lisette/commit/c123f33fc5c674d96dff66f60622e9bb802b4059)
- fix: prevent subtraction overflow in span calculation [`b47b218`](https://github.com/ivov/lisette/commit/b47b2180bde5b112f6c2365c2f4ad94431c0e61c)
- fix: remove unnecessary borrow in nil diagnostic format [`7e576be`](https://github.com/ivov/lisette/commit/7e576beaee77f68f46093327941a05d0ad39ed31)
- refactor: improve CLI help consistency and hide internal commands [`b0aa140`](https://github.com/ivov/lisette/commit/b0aa14063ef1117fa3feb8708ecd08b7348b0032)
- fix: improve doc help text colors, examples, and description [`ac3554a`](https://github.com/ivov/lisette/commit/ac3554a6e7003271412ff3fe937aedacfb7d58cb)
- feat: add quickstart link to CLI help and redirect page [`62ef1fe`](https://github.com/ivov/lisette/commit/62ef1fe5b53c90a51d4cae35b34d15e21a730c05)
- feat: show nil diagnostic for null, Nil, and undefined [`29f68a0`](https://github.com/ivov/lisette/commit/29f68a0ecef93afc3630c5939943a7765e062d1d)
- chore: update fuzz lockfile to workspace version 0.1.1 [`612b97c`](https://github.com/ivov/lisette/commit/612b97cf241f839a48461d6d1ba1e2cf6b73bc09)
- fix: lower parser max depth to 64 to prevent stack overflow [`1ab2b6c`](https://github.com/ivov/lisette/commit/1ab2b6cff453f6484dc504e5e09debcf8048b3f5)
- ci: enable changelog for main crate with cross-crate commits [`32d8819`](https://github.com/ivov/lisette/commit/32d8819407e7ce7f0bdf622258fcdb89d7509bb1)
- docs: open GitHub links in new tab and clean up repo URL [`809a73f`](https://github.com/ivov/lisette/commit/809a73f21e74349bce4b5a41276fdbd62b885736)
- ci: only create git tags and releases for main crate [`de15d96`](https://github.com/ivov/lisette/commit/de15d96e0dd75985da76d9cc9556572adda27191)
- docs: remove stray middot [`95589ab`](https://github.com/ivov/lisette/commit/95589ab87bea73c605d4559d54c1be95d109bc81)
- docs: trim unused font weights from Google Fonts request [`6b5c5f3`](https://github.com/ivov/lisette/commit/6b5c5f358431434fe47a4aca96807c7db810d0e8)
- docs: make homepage mobile-responsive [`b3c7dad`](https://github.com/ivov/lisette/commit/b3c7dad8b4676a6b9c810ce5587eb331718ea620)
- ci: restore release-plz prepare job and push trigger [`6789dab`](https://github.com/ivov/lisette/commit/6789dabb023746d54f60c94424f98cbe942600bf)
- fix: lower parser max depth to prevent stack overflow under asan [`97ebe8b`](https://github.com/ivov/lisette/commit/97ebe8bd7a3473aae8febf7b023a8bef883763b4)

## [0.1.1](https://github.com/ivov/lisette/compare/lisette-v0.1.0...lisette-v0.1.1) - 2026-03-21

- chore: bump version to 0.1.1 [`318e9a4`](https://github.com/ivov/lisette/commit/318e9a4093c8c47c87b9aa916a019bb066c317ff)
- chore: add readme path for crates.io [`95ecb00`](https://github.com/ivov/lisette/commit/95ecb009a8c174070a9e7a407facd406184bebb8)
- fix: ensure complete go.sum before running go build [`316b799`](https://github.com/ivov/lisette/commit/316b7993cc2b7edbb9d23b6577f207d95dec1612)
- docs: fix neovim plugin installation instructions [`fa234a6`](https://github.com/ivov/lisette/commit/fa234a62c20b2b595d8c59895f709d4870554b95)
- chore: include bindgen go.mod in version sync check [`fce89ab`](https://github.com/ivov/lisette/commit/fce89ab29cdad0d12dc9727f45aa82bd146ee8dc)
- refactor: move go version to standalone file [`6d61563`](https://github.com/ivov/lisette/commit/6d61563c8686e761d8bb75ce7ddc038abd0a1f5a)
- fix: resolve prelude path for crates.io packaging [`c8b0960`](https://github.com/ivov/lisette/commit/c8b09606eebc7ec01d9df1d75b6169f738e14a5d)
- chore: update zed extension grammar rev [`64004e2`](https://github.com/ivov/lisette/commit/64004e2d1b97c1e33ec3204ffb0d4028bef3c488)

## [0.1.0](https://github.com/ivov/lisette/releases/tag/lisette-v0.1.0) - 2026-03-21

- feat: initial release v0.1.0 [`a2fbd9d`](https://github.com/ivov/lisette/commit/a2fbd9d956ba38f52a456c5ad51da30e4bacdd1f)


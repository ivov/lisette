# Changelog

Lisette is under active development. Any version before 1.0.0 may include breaking changes.

## [0.3.4](https://github.com/ivov/lisette/compare/lisette-v0.3.3...lisette-v0.3.4) - 2026-06-11

- docs: document embedding imported Go types [#695](https://github.com/ivov/lisette/pull/695) [`d027dbd`](https://github.com/ivov/lisette/commit/d027dbd8ec971099ddf9961b01c36fc205c2c4de)
- fix: adjust diagnostics for `var` [#694](https://github.com/ivov/lisette/pull/694) [`8617a15`](https://github.com/ivov/lisette/commit/8617a159bc39d8e924a95bad3fb675d655ee5e41)
- feat: promote methods through unexported Go embeds [#693](https://github.com/ivov/lisette/pull/693) [`14e9316`](https://github.com/ivov/lisette/commit/14e9316e8671b9bff0dfa2bc73a2b2302d3a4570)
- fix: enforce sealing of imported Go interfaces [#692](https://github.com/ivov/lisette/pull/692) [`b1db049`](https://github.com/ivov/lisette/commit/b1db049e9ea419202a1ce8e915fa06015891eb55)
- test: render snapshot descriptions as yaml literal blocks [#691](https://github.com/ivov/lisette/pull/691) [`ff2c70a`](https://github.com/ivov/lisette/commit/ff2c70a8c33826036d1ceacfca74f8c127080a71)
- feat: add `lis emit` to CLI [#689](https://github.com/ivov/lisette/pull/689) [`f130d63`](https://github.com/ivov/lisette/commit/f130d637e2e92effb117d373f0de530ae1faaa27)
- refactor: reshape `lis run` onto build-then-exec [#688](https://github.com/ivov/lisette/pull/688) [`bd807f9`](https://github.com/ivov/lisette/commit/bd807f9c8feadc49745987782d37ce62ae90073d)
- feat: broaden imported types accepted as embed targets [#687](https://github.com/ivov/lisette/pull/687) [`a3edafa`](https://github.com/ivov/lisette/commit/a3edafaee78dbcfb70a1b7e2bfd1f00c229d0b33)
- chore: upgrade to `insta` 1.48.0 [#686](https://github.com/ivov/lisette/pull/686) [`eb77a81`](https://github.com/ivov/lisette/commit/eb77a815657c2c8322a183ee5fb99d0591f52c31)
- perf: parallelize module registration [#685](https://github.com/ivov/lisette/pull/685) [`d197b78`](https://github.com/ivov/lisette/commit/d197b784586764656ab9370612084da6635c416f)
- refactor: replace text-scanned liveness with structural tracking [#683](https://github.com/ivov/lisette/pull/683) [`35aa700`](https://github.com/ivov/lisette/commit/35aa700eddbb79300640c57501fc4e8fb372625d)
- fix: reject non-comparable interface and unknown equality [#682](https://github.com/ivov/lisette/pull/682) [`ddd6bc0`](https://github.com/ivov/lisette/commit/ddd6bc0a77c406a3594b38ba87c1c8e855362a0f)
- refactor: de-flatten imported struct embeds [#681](https://github.com/ivov/lisette/pull/681) [`1bd8dfc`](https://github.com/ivov/lisette/commit/1bd8dfc792cfc4cd08c6161c4cee5a4d7096f11c)
- feat: broaden match-to-if-let lint coverage [#680](https://github.com/ivov/lisette/pull/680) [`244fe2a`](https://github.com/ivov/lisette/commit/244fe2a1d913a0984d43c06916a631413deca981)
- perf: dispatch ast walk checks by expression kind [#679](https://github.com/ivov/lisette/pull/679) [`202244a`](https://github.com/ivov/lisette/commit/202244a7b4508d756c45a27dff094ba355997be0)
- perf: memoize global enum layouts and generic constraints in emit [#677](https://github.com/ivov/lisette/pull/677) [`1827dc0`](https://github.com/ivov/lisette/commit/1827dc074299531db010307c295872ef8cb6e196)
- test: add imported types to embed harness [#676](https://github.com/ivov/lisette/pull/676) [`aeeca19`](https://github.com/ivov/lisette/commit/aeeca1901c8f80b540d7d2ccad7447fab31911bd)
- feat: embedding for generic structs [#674](https://github.com/ivov/lisette/pull/674) [`2fef804`](https://github.com/ivov/lisette/commit/2fef804c25897eb32ac48ed05d06fd21fa606a70)
- perf: freeze types in place instead of rebuilding the ast [#673](https://github.com/ivov/lisette/pull/673) [`156652c`](https://github.com/ivov/lisette/commit/156652c8a3b90159ea8980d2fa8a158ac67c04d2)
- fix: reject namespace used as a value [#671](https://github.com/ivov/lisette/pull/671) [`c9e1eec`](https://github.com/ivov/lisette/commit/c9e1eecd2a257d019dddbacd798c81999e9a981d)
- feat: hover and go-to-definition on promoted members [#667](https://github.com/ivov/lisette/pull/667) [`f181b25`](https://github.com/ivov/lisette/commit/f181b25a1bd16f7978aa51cc77b1ec1123934a0c)

## [0.3.3](https://github.com/ivov/lisette/compare/lisette-v0.3.2...lisette-v0.3.3) - 2026-06-08

- feat: support embedding in native structs [#666](https://github.com/ivov/lisette/pull/666) [`eec2ea2`](https://github.com/ivov/lisette/commit/eec2ea2e9154af0209afefe183f9522eb7c4e2aa)
- refactor: replace `impl` with `embed` for interface embedding [#664](https://github.com/ivov/lisette/pull/664) [`18d7902`](https://github.com/ivov/lisette/commit/18d79026991e050a0cd5374446afe516e96faabf)
- test: differential harness for struct and interface embedding [#663](https://github.com/ivov/lisette/pull/663) [`b580f92`](https://github.com/ivov/lisette/commit/b580f923dd102c51add906243c1f33ed22770e76)
- fix: skip `manual_map_or` lint on side-effecting match arms [#662](https://github.com/ivov/lisette/pull/662) [`ecb5ed8`](https://github.com/ivov/lisette/commit/ecb5ed88879a4df169d36771bdc4787898e4a66f)
- fix: type unexported singleton vars by their implemented interface [#661](https://github.com/ivov/lisette/pull/661) [`be8e0f2`](https://github.com/ivov/lisette/commit/be8e0f25064fd1c7b4c8d396ab1f6557929f3718)
- fix: prevent go types from shadowing prelude generics [#660](https://github.com/ivov/lisette/pull/660) [`e0f036b`](https://github.com/ivov/lisette/commit/e0f036b24ae7720562e957c6f58d39b16abc14d6)
- feat: add `Result.wrap_err` to attach context to errors [#658](https://github.com/ivov/lisette/pull/658) [`668cab7`](https://github.com/ivov/lisette/commit/668cab75790dec794d65bbd90c50d5216f4cf0f0)
- fix: keep f-string interpolations single-line [#657](https://github.com/ivov/lisette/pull/657) [`ee4f66a`](https://github.com/ivov/lisette/commit/ee4f66a18635437ec5dc7013332ee675d8d4fa9e)

## [0.3.2](https://github.com/ivov/lisette/compare/lisette-v0.3.1...lisette-v0.3.2) - 2026-06-07

- fix: cross-module reference tracking in unused lints [#651](https://github.com/ivov/lisette/pull/651) [`64b9dc7`](https://github.com/ivov/lisette/commit/64b9dc7568da079be41bd55e30fa6bfcec6689a2)
- refactor: introduce `embed` keyword [#650](https://github.com/ivov/lisette/pull/650) [`cd78bfb`](https://github.com/ivov/lisette/commit/cd78bfb99d3cced87bbc8a5608056bf42d40730b)
- feat: lint for type limit comparison [#648](https://github.com/ivov/lisette/pull/648) [`f8757f5`](https://github.com/ivov/lisette/commit/f8757f544fe3ee4e2c6feb47ac0571b38d621799)
- fix: classify compound wrappers by qualified id [#646](https://github.com/ivov/lisette/pull/646) [`192df75`](https://github.com/ivov/lisette/commit/192df7584e18dfd9d2a5b6dc99c50b9415a8e357)
- refactor: store type attributes as a map [#647](https://github.com/ivov/lisette/pull/647) [`7063de2`](https://github.com/ivov/lisette/commit/7063de2914f225f5d5dcfbc88b55ee673b8d22d3)
- feat: bind go anonymous struct types [#644](https://github.com/ivov/lisette/pull/644) [`3a44617`](https://github.com/ivov/lisette/commit/3a44617f42e92ef9f78f30f68242b82ecd4f9a49)
- feat: add redundant else lint [#643](https://github.com/ivov/lisette/pull/643) [`8f8b584`](https://github.com/ivov/lisette/commit/8f8b584ccc306e5e97a3e02712c2fcf30a2cc62e)
- feat: add manual find simplification lint [#642](https://github.com/ivov/lisette/pull/642) [`6e44a27`](https://github.com/ivov/lisette/commit/6e44a2756a23130fc3425cb390b60e84bfe522ab)
- fix: tuple-struct and newtype pattern exhaustiveness [#641](https://github.com/ivov/lisette/pull/641) [`1fd5350`](https://github.com/ivov/lisette/commit/1fd5350668096e4b5db576fa2350fef326393fc0)
- test: move sync tests in-process [#640](https://github.com/ivov/lisette/pull/640) [`85d8c44`](https://github.com/ivov/lisette/commit/85d8c443f23f4269e0297043226637ad38351795)
- fix: zero-fill emit for newtype and tuple-struct fields [#637](https://github.com/ivov/lisette/pull/637) [`8b68e65`](https://github.com/ivov/lisette/commit/8b68e65e20fd477c0b3b879f929cac2700c638fe)

## [0.3.1](https://github.com/ivov/lisette/compare/lisette-v0.3.0...lisette-v0.3.1) - 2026-06-06

- feat: add map_or_else to Result [#633](https://github.com/ivov/lisette/pull/633) [`d5939f7`](https://github.com/ivov/lisette/commit/d5939f7ecfe7321a90ed017f9d0f95a693c75d74)
- feat: add manual map_or simplification lint [#632](https://github.com/ivov/lisette/pull/632) [`fc4e772`](https://github.com/ivov/lisette/commit/fc4e772e3a85623d773222d2014c7280fb984289)
- fix: exclude `#[display]` struct fields from the unused-field lint [#631](https://github.com/ivov/lisette/pull/631) [`100d37d`](https://github.com/ivov/lisette/commit/100d37d2a19011c62236d6c5dcfa4c5504cec7c2)
- fix: stop `lis doc` mangling third-party module paths [#630](https://github.com/ivov/lisette/pull/630) [`f376290`](https://github.com/ivov/lisette/commit/f3762909242c7c0ab95138efee5117952bb2ec19)
- fix: suggest `cmp.Ordered` bound when comparing unbounded generics [#629](https://github.com/ivov/lisette/pull/629) [`a17a67a`](https://github.com/ivov/lisette/commit/a17a67a8cf13452b8062376c522a3fd940102b6a)
- refactor: continue collapsing emit layer into IR [#628](https://github.com/ivov/lisette/pull/628) [`ae8ca07`](https://github.com/ivov/lisette/commit/ae8ca07654fe01d7188514559530b4f0bb3d7e4d)
- fix: resolve shadowed builtin types in type position [#627](https://github.com/ivov/lisette/pull/627) [`3109a37`](https://github.com/ivov/lisette/commit/3109a376efe5dceec9d3c66f36e25a23ac0a495b)
- fix: stack overflow when zero-filling a function-alias field [#626](https://github.com/ivov/lisette/pull/626) [`127d4e1`](https://github.com/ivov/lisette/commit/127d4e1e79f1bf6f25ce06211eae7c0925c509e4)
- feat: add collapsible if lint [#625](https://github.com/ivov/lisette/pull/625) [`fc77943`](https://github.com/ivov/lisette/commit/fc77943b875b76ce8f087933e2b8a2e461e79ad7)
- feat: reject legacy octal integer literals [#624](https://github.com/ivov/lisette/pull/624) [`94c1747`](https://github.com/ivov/lisette/commit/94c17471f433c8c96787d2584a174037137fb2f7)
- feat: add single-arm select lint [#622](https://github.com/ivov/lisette/pull/622) [`80e1837`](https://github.com/ivov/lisette/commit/80e183784dcb98257517070fe0cc90c2bb909c72)
- feat: add manual `strings.ReplaceAll` lint [#621](https://github.com/ivov/lisette/pull/621) [`2a88406`](https://github.com/ivov/lisette/commit/2a88406db09d5663ed3b1199f89d0ce6a37f0cfd)
- feat: add redundant slice bounds lint [#620](https://github.com/ivov/lisette/pull/620) [`eb6f3f4`](https://github.com/ivov/lisette/commit/eb6f3f40a796fc1574ed13ca1ba641d5919bb2e4)
- feat: add redundant `Sprintf` lint [#619](https://github.com/ivov/lisette/pull/619) [`e515873`](https://github.com/ivov/lisette/commit/e515873756b2820e66f455acd9df9f0b1c6e2000)
- refactor: dedupe emit coercion, call, and abi helpers [#618](https://github.com/ivov/lisette/pull/618) [`f3db058`](https://github.com/ivov/lisette/commit/f3db058646de9fba61392c13a86efc98f2b074fa)
- feat: add manual `bytes.Equal` lint [#616](https://github.com/ivov/lisette/pull/616) [`d909ca0`](https://github.com/ivov/lisette/commit/d909ca089fc681dd872bd3bf80a1ae234e03b9cd)
- docs: clarify interop directions [#617](https://github.com/ivov/lisette/pull/617) [`6b78dba`](https://github.com/ivov/lisette/commit/6b78dba579be5358c676a796b7ca24dbecfe0613)
- feat: add manual `time.Until` lint [#615](https://github.com/ivov/lisette/pull/615) [`1d8000d`](https://github.com/ivov/lisette/commit/1d8000d78bbac70679cf9bd96fc8bd603572cb47)
- feat: add manual `time.Since` lint [#614](https://github.com/ivov/lisette/pull/614) [`375a226`](https://github.com/ivov/lisette/commit/375a2266950de3696cb4f69457232e2f0a974fb5)
- feat: offer lsp attribute completions on `#[` [#613](https://github.com/ivov/lisette/pull/613) [`9f28fff`](https://github.com/ivov/lisette/commit/9f28fff98c1dc0b0b09a371b39c3c75e57daba12)
- refactor: make the scope stack the single source for return context [#611](https://github.com/ivov/lisette/pull/611) [`244a089`](https://github.com/ivov/lisette/commit/244a089b9568af2f014645597d1841465208a595)
- perf: share function signatures via Arc instead of deep-cloning [#610](https://github.com/ivov/lisette/pull/610) [`3687940`](https://github.com/ivov/lisette/commit/368794037ce152df9884964799c210ecad427fbd)
- feat: add inefficient comparison lint [#609](https://github.com/ivov/lisette/pull/609) [`d67fda2`](https://github.com/ivov/lisette/commit/d67fda275d412b64b73ed5ddf4d4492e4eba578c)
- fix: pin generic impl type params during interface satisfaction [#608](https://github.com/ivov/lisette/pull/608) [`f7b8201`](https://github.com/ivov/lisette/commit/f7b8201e242a9a141bb9c77faf15dd813a386ba9)
- perf: cache module field projections, skip redundant type resolves [#607](https://github.com/ivov/lisette/pull/607) [`267f64f`](https://github.com/ivov/lisette/commit/267f64f7c09e0c6e98567941a4bb4a8f5ebd759b)
- fix: enforce pointer-receiver rule through generic bounds [#606](https://github.com/ivov/lisette/pull/606) [`8c46015`](https://github.com/ivov/lisette/commit/8c46015a7f7bccc20dbb6ca1e1d21e05f779d04e)
- refactor: add misplaced attribute diagnostic [#605](https://github.com/ivov/lisette/pull/605) [`ae95f55`](https://github.com/ivov/lisette/commit/ae95f55bf300c8682f15772a309f5fc78926c8d9)
- perf: stop cloning function and lambda signatures twice [#604](https://github.com/ivov/lisette/pull/604) [`63987ec`](https://github.com/ivov/lisette/commit/63987ec10814beb5d4a021c801bc06ab0a56b0f3)
- refactor: rebuild tree-sitter parser if stale in nvim [#603](https://github.com/ivov/lisette/pull/603) [`84b56f6`](https://github.com/ivov/lisette/commit/84b56f61afbbd2f99c32a517106504af0a2bda87)
- feat: add goos/goarch unknown-value comparison lint [#602](https://github.com/ivov/lisette/pull/602) [`9e7a38f`](https://github.com/ivov/lisette/commit/9e7a38ff65211613bd3cb1a902f3ea7ba9e70c90)
- perf: cut allocations in semantic analysis hot paths [#601](https://github.com/ivov/lisette/pull/601) [`cdfa21c`](https://github.com/ivov/lisette/commit/cdfa21c49623f8459cd9cf59e63f3e0b51531ff2)
- feat: add integer division to zero lint [#599](https://github.com/ivov/lisette/pull/599) [`814bbb3`](https://github.com/ivov/lisette/commit/814bbb316d116806a169b8d479d07a0814d0d043)

## [0.3.0](https://github.com/ivov/lisette/compare/lisette-v0.2.17...lisette-v0.3.0) - 2026-06-04

- refactor: stop threading store through inference signatures [#597](https://github.com/ivov/lisette/pull/597) [`4890ad5`](https://github.com/ivov/lisette/commit/4890ad5be004222b4dfaf7f6ebe7ef8367e7874e)
- feat: add out-of-domain value lint for closed named primitives [#596](https://github.com/ivov/lisette/pull/596) [`c434c3a`](https://github.com/ivov/lisette/commit/c434c3ab45e929a2e1e53b1a386f00fdf4ca1297)
- refactor: share context and walk across checks and lints [#595](https://github.com/ivov/lisette/pull/595) [`40f5264`](https://github.com/ivov/lisette/commit/40f5264746c45021c1a54fd12bd9c230e56d0bd0)
- feat: add non-negative length comparison lint [#593](https://github.com/ivov/lisette/pull/593) [`c268079`](https://github.com/ivov/lisette/commit/c2680798dfb5ddc02339a2f3da0fec9128f47913)
- feat: catch x % 1 in redundant operation lint [#592](https://github.com/ivov/lisette/pull/592) [`7f26440`](https://github.com/ivov/lisette/commit/7f264408bfc85009116e1cb2afe15c4d00a3f3f4)
- ci: flag breaking changes in github release notes [#591](https://github.com/ivov/lisette/pull/591) [`823ba05`](https://github.com/ivov/lisette/commit/823ba05269cddfd1e39e80ca7ef317f53a96287e)
- refactor!: rename `--format` to `--output` in `lis check` [#590](https://github.com/ivov/lisette/pull/590) [`6b886fe`](https://github.com/ivov/lisette/commit/6b886fe9bae5d42939c9c9db999fd2f0c05a409d)
- refactor!: remove value enums [#588](https://github.com/ivov/lisette/pull/588) [`260463c`](https://github.com/ivov/lisette/commit/260463cff8b0cd6cb6f4de675ff2694b8aba99ba)
- feat: add redundant operation simplification lint [#589](https://github.com/ivov/lisette/pull/589) [`5b834b0`](https://github.com/ivov/lisette/commit/5b834b0b3c59e9e074e401946775d06da3a03989)
- feat: add negated equality simplification lint [#586](https://github.com/ivov/lisette/pull/586) [`6edc4bf`](https://github.com/ivov/lisette/commit/6edc4bffaf48817b82bac2b4dc91fbcc7c6c99fc)
- fix: lsp hover improvements [#583](https://github.com/ivov/lisette/pull/583) [`c536ba2`](https://github.com/ivov/lisette/commit/c536ba2d58d7d654e78ecd82570dbdd55a6e3c1b)
- feat: add let-and-return simplification lint [#585](https://github.com/ivov/lisette/pull/585) [`fde3b5b`](https://github.com/ivov/lisette/commit/fde3b5bb304ef9b918f1fb10d2df45151eac3a96)
- feat: lint match on bool that should be if/else [#584](https://github.com/ivov/lisette/pull/584) [`b08193d`](https://github.com/ivov/lisette/commit/b08193d3f80fd6b8fea54efb6b3f64a0dfc6d15f)
- feat: add match-to-let simplification lint [#582](https://github.com/ivov/lisette/pull/582) [`e27c9b7`](https://github.com/ivov/lisette/commit/e27c9b70b9cbf5e9c4ea100b451d8e7ddc1a161b)
- feat: add redundant assert_type lint [#580](https://github.com/ivov/lisette/pull/580) [`802bbfd`](https://github.com/ivov/lisette/commit/802bbfd03170e47679591d957081f235aaf567ba)
- refactor!: rename attributes to `#[iterate]` and `#[display]` [#578](https://github.com/ivov/lisette/pull/578) [`ba0503e`](https://github.com/ivov/lisette/commit/ba0503e982bf21b76afa27f4f2d931600bd09afa)

## [0.2.17](https://github.com/ivov/lisette/compare/lisette-v0.2.16...lisette-v0.2.17) - 2026-06-02

- feat: add manual emptiness check lint [#577](https://github.com/ivov/lisette/pull/577) [`6dd1cb1`](https://github.com/ivov/lisette/commit/6dd1cb16c933e952f5eaec1aff1e5bb915cce389)
- feat: add manual compound assignment lint [#576](https://github.com/ivov/lisette/pull/576) [`3e569d9`](https://github.com/ivov/lisette/commit/3e569d9a02b3cb8c3b9a53beac002acccc430acf)
- fix: make newtype structs and bool-backed named types nominal [#575](https://github.com/ivov/lisette/pull/575) [`c3b2f75`](https://github.com/ivov/lisette/commit/c3b2f7586e67829cc02e4bdb384586923f893c01)
- fix: lsp goto-def on type alias with same name as origin type name [#569](https://github.com/ivov/lisette/pull/569) [`801ce11`](https://github.com/ivov/lisette/commit/801ce118b5f3d58fe49391076bbe72197a164fa5)
- feat: gate stringer synthesis on `#[displayable]` [#574](https://github.com/ivov/lisette/pull/574) [`a9333a9`](https://github.com/ivov/lisette/commit/a9333a90d2fa4b2804e2d64fa7890d5cc4a8657b)
- fix: make string value enums nominal [#573](https://github.com/ivov/lisette/pull/573) [`6066322`](https://github.com/ivov/lisette/commit/6066322f6ad46d3ffa7656c092d1850aa31deb09)
- fix: make numeric value enums nominal [#572](https://github.com/ivov/lisette/pull/572) [`081757a`](https://github.com/ivov/lisette/commit/081757aec41dd92d4181dfc789f095a3f6476d60)
- feat: introduce `#[displayable]` attribute [#571](https://github.com/ivov/lisette/pull/571) [`d7acbed`](https://github.com/ivov/lisette/commit/d7acbedbe93a4466cd05404d1d78be4b20743d56)
- refactor: suggest WaitGroup.Go in waitgroup warning [#570](https://github.com/ivov/lisette/pull/570) [`190180b`](https://github.com/ivov/lisette/commit/190180b880b40f9552417c9f4f957d0febd2fb86)
- refactor: route stringer synthesis through a single gate [#568](https://github.com/ivov/lisette/pull/568) [`e60bde0`](https://github.com/ivov/lisette/commit/e60bde0d0731076688dda49366ed2983b1564679)
- refactor: set up opt-in stringer capability [#567](https://github.com/ivov/lisette/pull/567) [`1557c7c`](https://github.com/ivov/lisette/commit/1557c7cd37bfcb41cbe43d0dead61632c81b744f)
- fix: lsp autocomplete via non-generic type aliases [#563](https://github.com/ivov/lisette/pull/563) [`3ea55c0`](https://github.com/ivov/lisette/commit/3ea55c0d3bc9920d0a9fc352dda969e072294fac)
- refactor: reclassify diagnostics [#566](https://github.com/ivov/lisette/pull/566) [`afc5027`](https://github.com/ivov/lisette/commit/afc50275bbf94ddbd98a7929b17febed2387237f)
- feat: advisory `info` diagnostics [#565](https://github.com/ivov/lisette/pull/565) [`bb13b0a`](https://github.com/ivov/lisette/commit/bb13b0a1ab42dd48758df4a13c8e39b150e11024)
- feat: warn on lost url query mutation [#564](https://github.com/ivov/lisette/pull/564) [`a5f94c6`](https://github.com/ivov/lisette/commit/a5f94c60d530ef9943a2d26c48db921ebdb5c34d)
- feat: warn on match with identical arms [#562](https://github.com/ivov/lisette/pull/562) [`1a44853`](https://github.com/ivov/lisette/commit/1a448531a6b8f301a08538fc4a1d7c9c06a02e8a)
- feat: warn on closures replaceable by the function itself [#561](https://github.com/ivov/lisette/pull/561) [`07fd58a`](https://github.com/ivov/lisette/commit/07fd58a508743bca410be434fed5bfa7dcce0f0b)
- feat: add manual_map lint [#560](https://github.com/ivov/lisette/pull/560) [`4c0bcd3`](https://github.com/ivov/lisette/commit/4c0bcd32d6dbfb204d7c1709e1e7751cb7ff8069)
- feat: add manual_unwrap_or lint [#558](https://github.com/ivov/lisette/pull/558) [`0c19819`](https://github.com/ivov/lisette/commit/0c198199910e2e1512b143e39c74c4c13431fd8a)

## [0.2.16](https://github.com/ivov/lisette/compare/lisette-v0.2.15...lisette-v0.2.16) - 2026-05-31

- fix: reject value names in type annotations [#555](https://github.com/ivov/lisette/pull/555) [`c4dabbd`](https://github.com/ivov/lisette/commit/c4dabbd9d1a400cdcb08557b4874c5e193154696)
- feat: add `#[iterable]` for enum iteration [#557](https://github.com/ivov/lisette/pull/557) [`6a0db29`](https://github.com/ivov/lisette/commit/6a0db29d8bed754e9125196a243121ed220850ea)
- refactor: share ufcs methods across inference [#556](https://github.com/ivov/lisette/pull/556) [`f4a635b`](https://github.com/ivov/lisette/commit/f4a635b0648bc7f052ff4622730a0d857fa35244)
- fix: emit remaining pattern checks in switch case bodies [#554](https://github.com/ivov/lisette/pull/554) [`0cd4e5f`](https://github.com/ivov/lisette/commit/0cd4e5fe834aaa5967293b2cd1ef3c6a5283ebdf)
- feat: suggest is_some/is_ok over match-to-bool [#552](https://github.com/ivov/lisette/pull/552) [`0a34bf2`](https://github.com/ivov/lisette/commit/0a34bf29fddac6740e7c62fdb2bdf825ec447119)
- fix: validate unknown attributes on enums [#551](https://github.com/ivov/lisette/pull/551) [`00fcbf4`](https://github.com/ivov/lisette/commit/00fcbf48de22d2acb1850f3b96fe389052616e5f)
- refactor: render enums as bare variant names [#550](https://github.com/ivov/lisette/pull/550) [`4d817a4`](https://github.com/ivov/lisette/commit/4d817a4c308503169929fd80efca69c6034aacf5)
- feat: warn on range loop that only indexes a slice [#549](https://github.com/ivov/lisette/pull/549) [`69e5f43`](https://github.com/ivov/lisette/commit/69e5f43d04974b9cc4343b960b3dc53423b9e8e2)
- perf: box function variant to shrink type enum [#548](https://github.com/ivov/lisette/pull/548) [`9b679d9`](https://github.com/ivov/lisette/commit/9b679d94ce1011ba6c4b4721f8c1adf4a0a973f5)
- fix: reject enum and module types used as runtime values [#532](https://github.com/ivov/lisette/pull/532) [`30a5e73`](https://github.com/ivov/lisette/commit/30a5e7303b39f19534c361937df2f3df00849f98)
- feat: warn on loop that runs at most once [#545](https://github.com/ivov/lisette/pull/545) [`d3b1d9e`](https://github.com/ivov/lisette/commit/d3b1d9e923ac008a661a2dd654e39dee9ec59ac6)
- perf: avoid per-node allocations in ast traversal [#544](https://github.com/ivov/lisette/pull/544) [`e6ca8c2`](https://github.com/ivov/lisette/commit/e6ca8c2f3d5f43db1c1b1eba2dadcc0a4510bb6d)
- test: check lis learn project e2e [#543](https://github.com/ivov/lisette/pull/543) [`6dff023`](https://github.com/ivov/lisette/commit/6dff0239d9bdcdb0b0e7319855cf5adc00ec57e5)
- refactor: consolidate context-free checks onto shared visitor [#542](https://github.com/ivov/lisette/pull/542) [`a116b4b`](https://github.com/ivov/lisette/commit/a116b4b7cde0cfc5a2abde8ff8ed5c59aef946f7)
- test: build no-entry emit snapshots in e2e suite [#541](https://github.com/ivov/lisette/pull/541) [`b0bae23`](https://github.com/ivov/lisette/commit/b0bae2333efb5f324cd9d2ab22ec513a32295e8e)
- feat: reject loop with unchanging condition [#540](https://github.com/ivov/lisette/pull/540) [`80a0f1e`](https://github.com/ivov/lisette/commit/80a0f1ea8f6fb1f784d3817d439b19e4a1ffa8b2)
- test: check emitted Go with go vet [#539](https://github.com/ivov/lisette/pull/539) [`09d6c96`](https://github.com/ivov/lisette/commit/09d6c9638fffbdd933c97a5a58a26ae28b32d5fa)
- refactor: unify unit enum variant emission via constructor [#538](https://github.com/ivov/lisette/pull/538) [`a694be0`](https://github.com/ivov/lisette/commit/a694be0236644142b95cd589ba3e52e0cd84dcb0)
- fix: resolve enum variant access through a type alias [#537](https://github.com/ivov/lisette/pull/537) [`701d20b`](https://github.com/ivov/lisette/commit/701d20be7f10b3b3f7d609cbac0dfff117748bc9)
- ci: run in-crate unit tests [#536](https://github.com/ivov/lisette/pull/536) [`bde0e25`](https://github.com/ivov/lisette/commit/bde0e254d35aa72830c17379893e0dc02f66c5c8)
- feat: warn on WaitGroup.Add inside a task [#535](https://github.com/ivov/lisette/pull/535) [`186ed22`](https://github.com/ivov/lisette/commit/186ed221df9f9379c3438a6d52056c50cbe24142)
- feat: warn on needless boolean if-else [#534](https://github.com/ivov/lisette/pull/534) [`36a2a2a`](https://github.com/ivov/lisette/commit/36a2a2ad158426cb512e782ee6ac67d5c11837a2)
- fix: stop flattening tuple payloads of fallible calls [#533](https://github.com/ivov/lisette/pull/533) [`611906c`](https://github.com/ivov/lisette/commit/611906c179e6259f7b22b86ab0403ba014c82e0b)
- feat: warn on needless return in tail position [#530](https://github.com/ivov/lisette/pull/530) [`2128a29`](https://github.com/ivov/lisette/commit/2128a296dfa4ef10af47c70ee0363b6e9b2b4b56)

## [0.2.15](https://github.com/ivov/lisette/compare/lisette-v0.2.14...lisette-v0.2.15) - 2026-05-28

- fix: emit middle index for capped range-from slice [#527](https://github.com/ivov/lisette/pull/527) [`00f3b56`](https://github.com/ivov/lisette/commit/00f3b562a9e0887655766e7ebaf75828fe85da72)
- fix: enforce consistent evaluation order in emit [#526](https://github.com/ivov/lisette/pull/526) [`749ada5`](https://github.com/ivov/lisette/commit/749ada5067df054f62a08e4a563621aaa6450fb5)
- feat: reject channel and function fields in json types [#525](https://github.com/ivov/lisette/pull/525) [`317647b`](https://github.com/ivov/lisette/commit/317647b224de687e1d451ef3b9c75049235382ac)
- fix: evaluate deferred native-method operands at defer site [#524](https://github.com/ivov/lisette/pull/524) [`c905f1c`](https://github.com/ivov/lisette/commit/c905f1cc94e630b6324c315bc51eec9cb937106a)
- feat: flag charset misuse in strings.Trim calls [#523](https://github.com/ivov/lisette/pull/523) [`b6b3d43`](https://github.com/ivov/lisette/commit/b6b3d43ec4c61ddffa9f5edd5c12b0b97f755276)
- ci: harden release body and notes generation [#522](https://github.com/ivov/lisette/pull/522) [`5dd023e`](https://github.com/ivov/lisette/commit/5dd023e89b9d970075a95f64af883a57a5159cd4)
- docs: restore blank lines between changelog releases [#521](https://github.com/ivov/lisette/pull/521) [`e9f267e`](https://github.com/ivov/lisette/commit/e9f267e6c3a0485c900ceca46f58a108d2d68544)
- feat: flag duplicate args in stdlib calls [#520](https://github.com/ivov/lisette/pull/520) [`6b5e3e4`](https://github.com/ivov/lisette/commit/6b5e3e4199f03a92d6213dae3db6547274483a4b)
- feat: reject decimal file mode literals [#518](https://github.com/ivov/lisette/pull/518) [`b8a2bdd`](https://github.com/ivov/lisette/commit/b8a2bdd45ecd31c59b52edbe0a43effbbc5c24e7)
- fix: track function return context for nested ? propagation [#517](https://github.com/ivov/lisette/pull/517) [`00316bf`](https://github.com/ivov/lisette/commit/00316bf8f68467895407f9868800fca414867afd)

## [0.2.14](https://github.com/ivov/lisette/compare/lisette-v0.2.13...lisette-v0.2.14) - 2026-05-27

- fix: translate `unknown` to any in explicit type arguments [#513](https://github.com/ivov/lisette/pull/513) [`7d6687a`](https://github.com/ivov/lisette/commit/7d6687a8ae52f122aef8b6341d3de707d01a667f)
- feat: reject empty select default arm in loop [#515](https://github.com/ivov/lisette/pull/515) [`68e1881`](https://github.com/ivov/lisette/commit/68e1881b9cb1905041103f6debf0a17e52f7088d)
- ci: split lsp tests from main suite [#514](https://github.com/ivov/lisette/pull/514) [`470d511`](https://github.com/ivov/lisette/commit/470d511e07b68f3e20ae5f796f44632e24557687)
- ci: drop redundant job, cache golangci-lint, run lsp tests [#512](https://github.com/ivov/lisette/pull/512) [`45c2388`](https://github.com/ivov/lisette/commit/45c2388fdcbe040ae16e495b9577ee185593da31)
- feat: reject empty infinite loop [#511](https://github.com/ivov/lisette/pull/511) [`fafb988`](https://github.com/ivov/lisette/commit/fafb988a5472e6a2f6b0354c1da865a9a0fccb1c)
- feat: reject repeated condition in if else if chain [#510](https://github.com/ivov/lisette/pull/510) [`024aab4`](https://github.com/ivov/lisette/commit/024aab48996d8c84c89a7fb9d0478bb64de8e05f)
- feat: reject shift exceeding integer width [#509](https://github.com/ivov/lisette/pull/509) [`b45378f`](https://github.com/ivov/lisette/commit/b45378f047a8c5d30bb9a0f060057507937796ae)
- fix: unwrap nested option-slice at Go boundary [#508](https://github.com/ivov/lisette/pull/508) [`4500702`](https://github.com/ivov/lisette/commit/45007023f21ce97cee49e72f593ff3a0b7a7a22a)
- fix: preserve type args from non-generic alias in struct calls [#507](https://github.com/ivov/lisette/pull/507) [`e9f7161`](https://github.com/ivov/lisette/commit/e9f71616682b06f337e4a294dcbb4ac2054a37df)
- feat: reject out-of-bounds slice indexing [#506](https://github.com/ivov/lisette/pull/506) [`7ab94ec`](https://github.com/ivov/lisette/commit/7ab94ecc68bd904c6c0dd4172221d59d08c60487)
- perf: avoid cloning function definition in non-builtin emit path [#505](https://github.com/ivov/lisette/pull/505) [`560678c`](https://github.com/ivov/lisette/commit/560678cb186f7c1c4f3fdf81534d4c93e7e2bc1c)
- fix: preserve type args on generic interface bounds in bindgen [#503](https://github.com/ivov/lisette/pull/503) [`39ac1b7`](https://github.com/ivov/lisette/commit/39ac1b7729026259d221b3430250277547d6eb83)
- perf: skip desugar pass when parser produced no desugarables [#502](https://github.com/ivov/lisette/pull/502) [`af8156c`](https://github.com/ivov/lisette/commit/af8156c1531645da32fcd39154ee7f0c52f8056f)

## [0.2.13](https://github.com/ivov/lisette/compare/lisette-v0.2.12...lisette-v0.2.13) - 2026-05-26

- perf: skip synthetic EOF in token lookup [#500](https://github.com/ivov/lisette/pull/500) [`c9bbdeb`](https://github.com/ivov/lisette/commit/c9bbdeb9f7fb83108d46780998b8c497c57b844f)
- feat: reject empty range [#499](https://github.com/ivov/lisette/pull/499) [`d74f193`](https://github.com/ivov/lisette/commit/d74f1930e60bf2ec8ccd83f03a72baf794481fcc)
- perf: introduce simple benchmarking setup [#498](https://github.com/ivov/lisette/pull/498) [`47dd262`](https://github.com/ivov/lisette/commit/47dd2624ba678495305b6a2609f7b80d4d351c18)
- feat: reject comparison against math.NaN() [#497](https://github.com/ivov/lisette/pull/497) [`e853566`](https://github.com/ivov/lisette/commit/e8535667b80a4e00965bc811d145e51e7f055a6c)
- feat: warn on verbose failure propagation [#496](https://github.com/ivov/lisette/pull/496) [`0aef44a`](https://github.com/ivov/lisette/commit/0aef44a04095c82dd12f80b2ce34a89b1c9bd7b6)
- refactor: consolidate in-memory loader impls [#495](https://github.com/ivov/lisette/pull/495) [`207da7f`](https://github.com/ivov/lisette/commit/207da7f4f8a7cb7269c495cc9d649a7dd55e1dda)
- refactor: prep lint setup for upcoming diagnostics [#494](https://github.com/ivov/lisette/pull/494) [`f0b06b6`](https://github.com/ivov/lisette/commit/f0b06b6743283d09815cc33e98b8f889c720d62d)
- feat: warn on invisible and bidi chars in strings [#493](https://github.com/ivov/lisette/pull/493) [`24abe4c`](https://github.com/ivov/lisette/commit/24abe4c9263cded785880f4bf1a5a9dbd7900b48)
- fix: guard interface parent walks against embedding cycles [#492](https://github.com/ivov/lisette/pull/492) [`a1e2001`](https://github.com/ivov/lisette/commit/a1e20019d2468130e0a732f80ee681eb61f7701e)
- fix: complete interface methods in lsp [#491](https://github.com/ivov/lisette/pull/491) [`079daa1`](https://github.com/ivov/lisette/commit/079daa1a954557818db5c6465a6ae14cc991e79f)
- feat: reject deferring a mutex lock [#490](https://github.com/ivov/lisette/pull/490) [`04a8a2b`](https://github.com/ivov/lisette/commit/04a8a2bdcf72775d24317428a947b17ff1c34f04)
- ci: catch lockfile drift [#489](https://github.com/ivov/lisette/pull/489) [`b64b576`](https://github.com/ivov/lisette/commit/b64b576d3ef16f6f806487991ea4d594d872d814)
- chore: update lockfile [#487](https://github.com/ivov/lisette/pull/487) [`7c175d2`](https://github.com/ivov/lisette/commit/7c175d267f36f04ce65d1beade181f6f9e15225e)
- feat: warn on unsigned integer compared against zero [#486](https://github.com/ivov/lisette/pull/486) [`d3fb542`](https://github.com/ivov/lisette/commit/d3fb542aeb214a173fb411e8b658247a86f22071)
- refactor: restructure emit layer [#484](https://github.com/ivov/lisette/pull/484) [`f4757af`](https://github.com/ivov/lisette/commit/f4757af591949eb4650be4d1791c5cb762656740)

## [0.2.12](https://github.com/ivov/lisette/compare/lisette-v0.2.11...lisette-v0.2.12) - 2026-05-25

- fix: disambiguate EcoString as_ref calls with as_str [#482](https://github.com/ivov/lisette/pull/482) [`9f16eef`](https://github.com/ivov/lisette/commit/9f16eef49df5a174c9139656ec5e128611758ec1)

## [0.2.11](https://github.com/ivov/lisette/compare/lisette-v0.2.10...lisette-v0.2.11) - 2026-05-23

- fix: reject public types with non-exportable names [#480](https://github.com/ivov/lisette/pull/480) [`2226da8`](https://github.com/ivov/lisette/commit/2226da8489f515d510048cb6cd9bf69dc65d3476)
- fix: reject any/comparable as type or generic names [#479](https://github.com/ivov/lisette/pull/479) [`d3ed928`](https://github.com/ivov/lisette/commit/d3ed9280f8ff6a11cd4563eda66c316d87c0f791)
- fix: diagnose duplicate json methods on #[json] enums [#477](https://github.com/ivov/lisette/pull/477) [`5fcc6b0`](https://github.com/ivov/lisette/commit/5fcc6b08c6c0bef2172f99325d0c686fe88d4b89)
- ci: drop release.yml fork guard [#476](https://github.com/ivov/lisette/pull/476) [`865b74c`](https://github.com/ivov/lisette/commit/865b74c4af9329d36f1ccf7134fd927c2eb06a1d)

## [0.2.10](https://github.com/ivov/lisette/compare/lisette-v0.2.9...lisette-v0.2.10) - 2026-05-23

- fix: bind more value-plus-error returns as Partial [#475](https://github.com/ivov/lisette/pull/475) [`e1c3772`](https://github.com/ivov/lisette/commit/e1c377254a1bd844d49fb5d4297fc362fb76b70c)
- fix: bind WriteString, ReadFrom and WriteTo as Partial [#474](https://github.com/ivov/lisette/pull/474) [`36ab5d3`](https://github.com/ivov/lisette/commit/36ab5d33b4907b8f92073bbc2e4e0bc658191e38)
- fix: interface not implemented diagnostic for wrapper types [#473](https://github.com/ivov/lisette/pull/473) [`25b2035`](https://github.com/ivov/lisette/commit/25b20359005700d4676b65e5652b5375a780612a)
- test: gate e2e suite on zero re-emit failures [#472](https://github.com/ivov/lisette/pull/472) [`2dfb714`](https://github.com/ivov/lisette/commit/2dfb714a3fb56107ad8f7923aa2b00ca3da8cccb)
- fix: bound global prelude and stdlib defs cache [#471](https://github.com/ivov/lisette/pull/471) [`bb0bf4f`](https://github.com/ivov/lisette/commit/bb0bf4ffab25bd397ea3b9d4186510ee40d502f0)
- feat: unix diagnostic format for lis check [#470](https://github.com/ivov/lisette/pull/470) [`308e859`](https://github.com/ivov/lisette/commit/308e85939326a85e0b5caf053c4dbac0e60462dd)
- fix: prune go output and caches for removed modules [#469](https://github.com/ivov/lisette/pull/469) [`d6a2232`](https://github.com/ivov/lisette/commit/d6a2232f88a03f12a06863c982ce53756ddbd729)
- ci: skip fuzz and release workflows on forks [#468](https://github.com/ivov/lisette/pull/468) [`2a8a152`](https://github.com/ivov/lisette/commit/2a8a152290163a86faa7b5086415f4448ade0a49)
- feat: relative paths in diagnostics [#467](https://github.com/ivov/lisette/pull/467) [`39830a0`](https://github.com/ivov/lisette/commit/39830a045e3c9c1023fadf69f975e028d9450b31)
- feat: bare variants in match arms [#465](https://github.com/ivov/lisette/pull/465) [`2faa0f6`](https://github.com/ivov/lisette/commit/2faa0f6bbcecefc20bcdb7cfb767c8335a72260a)

## [0.2.9](https://github.com/ivov/lisette/compare/lisette-v0.2.8...lisette-v0.2.9) - 2026-05-20

- fix: drop redundant block around irrefutable match arm [#464](https://github.com/ivov/lisette/pull/464) [`6e3e211`](https://github.com/ivov/lisette/commit/6e3e2112860d3183ecf51757987ae9e85a3b5fc5)
- fix: derive generic method call shapes from declarations [#463](https://github.com/ivov/lisette/pull/463) [`b4c22b5`](https://github.com/ivov/lisette/commit/b4c22b593da837f60698ea513b1c45ee4ec84ff9)
- fix: classify generic callee shape from its declaration [#461](https://github.com/ivov/lisette/pull/461) [`07914a1`](https://github.com/ivov/lisette/commit/07914a1bda37687197c4fe254e01f567324b003b)

## [0.2.8](https://github.com/ivov/lisette/compare/lisette-v0.2.7...lisette-v0.2.8) - 2026-05-19

- fix: adapt lowered fn arg shape at generic call boundary [#459](https://github.com/ivov/lisette/pull/459) [`ee69def`](https://github.com/ivov/lisette/commit/ee69deff9510071418c1ac66c104606af0507f36)
- fix: wrap go interface returns as option [#458](https://github.com/ivov/lisette/pull/458) [`79892a4`](https://github.com/ivov/lisette/commit/79892a426ed11a59df43329a0470214dfdf85e02)
- ci: require IDE extension version bump on grammar change [#457](https://github.com/ivov/lisette/pull/457) [`d6613a0`](https://github.com/ivov/lisette/commit/d6613a072757418585868cf5a65ec10fd3bb6a60)
- ci: add tree-sitter drift and test checks [#454](https://github.com/ivov/lisette/pull/454) [`5469274`](https://github.com/ivov/lisette/commit/5469274bebe8d452a8a10ed19233a55bd6593f1e)
- fix: bitwise tokens missing from tree-sitter parser [#453](https://github.com/ivov/lisette/pull/453) [`1748f26`](https://github.com/ivov/lisette/commit/1748f26d221c5f2f10f86cb3f7e7bfb373d9e7dc)
- fix: align Go interface and impl pointer-return nilability [#451](https://github.com/ivov/lisette/pull/451) [`dba4c74`](https://github.com/ivov/lisette/commit/dba4c741a38086fbae97d85e6f9f0fc170df3732)
- fix: terminate generics parser at eof [#450](https://github.com/ivov/lisette/pull/450) [`6a8d718`](https://github.com/ivov/lisette/commit/6a8d7184ec1646d067a10819742a81fc5c0ebe7f)
- chore: rebuild playground [#446](https://github.com/ivov/lisette/pull/446) [`126dd8e`](https://github.com/ivov/lisette/commit/126dd8e56aa62c66b6580a4b0536e8e3c30fb3c6)
- fix: allow mutually recursive function-type aliases [#445](https://github.com/ivov/lisette/pull/445) [`14ac9f4`](https://github.com/ivov/lisette/commit/14ac9f40e7a904ae1436aae970ca46397dffaa71)
- ci: guard release workflows from running on forks [#444](https://github.com/ivov/lisette/pull/444) [`7f9c583`](https://github.com/ivov/lisette/commit/7f9c58357fbdc4e882bc1c7c67dd6d8774554642)
- feat: bitwise operators [#382](https://github.com/ivov/lisette/pull/382) [`bb5de34`](https://github.com/ivov/lisette/commit/bb5de3434223f0ef4fa62cabf7ed11519c87dfdc)
- fix: qualify imported enum in variant-not-found diagnostic [#442](https://github.com/ivov/lisette/pull/442) [`4a02f60`](https://github.com/ivov/lisette/commit/4a02f60db605731729688f21644fd8b8c8754033)

## [0.2.7](https://github.com/ivov/lisette/compare/lisette-v0.2.6...lisette-v0.2.7) - 2026-05-17

- fix: unwrap option at go any and honor tail-return hints [#437](https://github.com/ivov/lisette/pull/437) [`afcf72c`](https://github.com/ivov/lisette/commit/afcf72ccfc6258e0c986de233cb299485c161f05)
- refactor: collect generic constraints before emit [#436](https://github.com/ivov/lisette/pull/436) [`7671020`](https://github.com/ivov/lisette/commit/7671020249d3b4606c31f7caa5ebc84e61b8419d)
- refactor: centralize alias-aware emit shape queries [#435](https://github.com/ivov/lisette/pull/435) [`7fcdfc3`](https://github.com/ivov/lisette/commit/7fcdfc390b5f367a349cbbf9b66b5b68761e7ade)
- fix: lower direct Err(...)? and None? propagation [#434](https://github.com/ivov/lisette/pull/434) [`9620f8a`](https://github.com/ivov/lisette/commit/9620f8a51e413ba67442e85c14f3924e0aa9e3f7)
- fix: suppress unused locals in fused Result match emit [#433](https://github.com/ivov/lisette/pull/433) [`cd93588`](https://github.com/ivov/lisette/commit/cd935888a6c984434625f2bcb7660592a3121ba2)
- fix: require screaming snake case for constants [#432](https://github.com/ivov/lisette/pull/432) [`ac5cdca`](https://github.com/ivov/lisette/commit/ac5cdca88f8ea3e1add257f9cec0c8b9fc4c663e)
- docs: mention mise install path [#431](https://github.com/ivov/lisette/pull/431) [`046b3b0`](https://github.com/ivov/lisette/commit/046b3b0c533393207c87a6bc1230198b1c23885b)
- docs: mention homebrew install path [#430](https://github.com/ivov/lisette/pull/430) [`19d6493`](https://github.com/ivov/lisette/commit/19d6493424cf01f7d28ac6db8786a51dbbf56f9d)
- fix: clearer error for typed binding missing initializer [#429](https://github.com/ivov/lisette/pull/429) [`ab431c9`](https://github.com/ivov/lisette/commit/ab431c9e0740e85b6a72dd6015d1f4edd5e5b819)
- fix: correct bindgen overrides for sourcegraph/conc [#428](https://github.com/ivov/lisette/pull/428) [`83297ba`](https://github.com/ivov/lisette/commit/83297ba35ecbcf8791d6692fad50cd8557a4b821)
- fix: suppress unused-value cascades on errored expressions [#427](https://github.com/ivov/lisette/pull/427) [`04a5822`](https://github.com/ivov/lisette/commit/04a5822d64855f6752b337e4877d13940d1ab5ae)
- fix: pointer receiver promotion in bindgen [#426](https://github.com/ivov/lisette/pull/426) [`96abad6`](https://github.com/ivov/lisette/commit/96abad60b42a6cbecb168c1adf97774d6ae42e2e)
- fix: detect map literal attempts at parse time [#425](https://github.com/ivov/lisette/pull/425) [`52508e7`](https://github.com/ivov/lisette/commit/52508e748ea2f671e5993b4bda1086f6cd379221)
- fix: suppress unused-field lint on pub fields [#424](https://github.com/ivov/lisette/pull/424) [`cc241a7`](https://github.com/ivov/lisette/commit/cc241a7b96eec4250dd77a80223278e558289546)
- refactor: reduce emit complexity [#423](https://github.com/ivov/lisette/pull/423) [`bcc574b`](https://github.com/ivov/lisette/commit/bcc574b373d4257cb0c4eee7c2295fecdbc2e3fd)
- docs: add section on typed nil at the boundary [#422](https://github.com/ivov/lisette/pull/422) [`045e502`](https://github.com/ivov/lisette/commit/045e502b90a54591aaffa763c366e39ba5295ea3)
- refactor: emit idiomatic fmt verbs in interpolations [#420](https://github.com/ivov/lisette/pull/420) [`5121f0d`](https://github.com/ivov/lisette/commit/5121f0de48c9bdd8f6c00bb73f5b6311918b9834)

## [0.2.6](https://github.com/ivov/lisette/compare/lisette-v0.2.5...lisette-v0.2.6) - 2026-05-16

- fix: resolve dotted go import paths [#419](https://github.com/ivov/lisette/pull/419) [`d22fdfa`](https://github.com/ivov/lisette/commit/d22fdfaa0bc78850c07c0130769379999294042f)
- fix: tighten bit-flag detection in bindgen [#418](https://github.com/ivov/lisette/pull/418) [`7a3c6fd`](https://github.com/ivov/lisette/commit/7a3c6fd20034195fb28d32175ba43b6cb2a0f306)
- refactor: restore emit cosmetics [#416](https://github.com/ivov/lisette/pull/416) [`57f60eb`](https://github.com/ivov/lisette/commit/57f60ebda8ad332de8e77434f97620a4f44b0136)
- fix: dedupe blank-import diagnostic [#415](https://github.com/ivov/lisette/pull/415) [`6aa882b`](https://github.com/ivov/lisette/commit/6aa882b04b58195f209113b23f151eaef1aed3eb)
- refactor: drop last cosmetic emit cleanup pass [#414](https://github.com/ivov/lisette/pull/414) [`8e41a3a`](https://github.com/ivov/lisette/commit/8e41a3a9195374606918f872b6352e4bca8b2e0d)
- refactor: drop four cosmetic emit cleanup passes [#413](https://github.com/ivov/lisette/pull/413) [`ed30007`](https://github.com/ivov/lisette/commit/ed300078ea67500655a10e7bfd5bfae205280b63)
- refactor: ast-level emit negation and fmt collapse [#411](https://github.com/ivov/lisette/pull/411) [`f0b3966`](https://github.com/ivov/lisette/commit/f0b3966580b23a38cd536211f181bbe8286c06a2)

## [0.2.5](https://github.com/ivov/lisette/compare/lisette-v0.2.4...lisette-v0.2.5) - 2026-05-14

- fix: track go import usage during emit [#410](https://github.com/ivov/lisette/pull/410) [`00d1b7a`](https://github.com/ivov/lisette/commit/00d1b7ab37daa2e0af24fb55e84622cb6e10a18c)
- perf: emit nil for empty Slice literals [#409](https://github.com/ivov/lisette/pull/409) [`1155da5`](https://github.com/ivov/lisette/commit/1155da531aa7e536c256c7460e611209f58e8e12)
- refactor: emit architecture overhaul [#407](https://github.com/ivov/lisette/pull/407) [`01c7c34`](https://github.com/ivov/lisette/commit/01c7c3443a8f7e41ed94fd6fc2dda8b082278d47)
- fix: cache no longer hides internal_type_leak warnings [#405](https://github.com/ivov/lisette/pull/405) [`d820e1d`](https://github.com/ivov/lisette/commit/d820e1d8472f35cd56a578d40c9c62d9d2b2329d)
- perf: skip unchanged emit/gofmt/tidy work on rebuilds [#403](https://github.com/ivov/lisette/pull/403) [`3ef0961`](https://github.com/ivov/lisette/commit/3ef096162bba93a595c84c711962607082a29295)

## [0.2.4](https://github.com/ivov/lisette/compare/lisette-v0.2.3...lisette-v0.2.4) - 2026-05-12

- fix: honor allow attributes on interface methods [#402](https://github.com/ivov/lisette/pull/402) [`3e2e5c2`](https://github.com/ivov/lisette/commit/3e2e5c2ddba766bd8a81c4451877845c2c1909f2)
- fix: shrink bindgen skip surface [#401](https://github.com/ivov/lisette/pull/401) [`56948a2`](https://github.com/ivov/lisette/commit/56948a23da241b967758b52ff8e8a3b268610ba7)
- refactor: consolidate fallible lowering and coercion dispatch [#400](https://github.com/ivov/lisette/pull/400) [`32325a2`](https://github.com/ivov/lisette/commit/32325a20238b0ffeb5ee8ed41d9be7ba494a6d5b)
- refactor: consolidate recurring emit patterns [#399](https://github.com/ivov/lisette/pull/399) [`a0a60ca`](https://github.com/ivov/lisette/commit/a0a60ca05251c62ff1a738956c16a5418a070742)
- fix: reject member access on uninferred receiver [#398](https://github.com/ivov/lisette/pull/398) [`3a0c98c`](https://github.com/ivov/lisette/commit/3a0c98c9e6e292300bb509c543eeea14a44b2940)
- refactor: polish diagnostics [#397](https://github.com/ivov/lisette/pull/397) [`941c61b`](https://github.com/ivov/lisette/commit/941c61b23ac7c2e95b7c8f962f1fdbcca1d8775e)
- fix: solve more emit edge cases [#396](https://github.com/ivov/lisette/pull/396) [`c738613`](https://github.com/ivov/lisette/commit/c738613d6acc63c5c65c3cc4906bbcc4d26dd857)
- fix: prevent emit corner cases [#395](https://github.com/ivov/lisette/pull/395) [`c656a66`](https://github.com/ivov/lisette/commit/c656a6651049fb358b8102023aeb7ca9db61cfe0)
- fix: prevent silent emit miscompilations [#394](https://github.com/ivov/lisette/pull/394) [`f786d47`](https://github.com/ivov/lisette/commit/f786d47902399bdbb40f14443a627e6b3f68cc28)
- fix: type-assert tuple/newtype patterns against go interfaces [#393](https://github.com/ivov/lisette/pull/393) [`f65d99a`](https://github.com/ivov/lisette/commit/f65d99a2ac9a1e312daec1867f77fe5205ecc24b)
- fix: prevent go const eligibility from leaking across scopes [#392](https://github.com/ivov/lisette/pull/392) [`f3a09b8`](https://github.com/ivov/lisette/commit/f3a09b80f4b7a24bba39c3e879e30a12b9adf1f7)
- fix: emit tail panic as statement in fallible function [#391](https://github.com/ivov/lisette/pull/391) [`5f77c90`](https://github.com/ivov/lisette/commit/5f77c90ffb73f18f723ba6fbded237880dfe85b0)
- fix: monomorphize generic interface adapters per instantiation [#389](https://github.com/ivov/lisette/pull/389) [`563c642`](https://github.com/ivov/lisette/commit/563c642cc89d8b475555d629d3d11c242d70f333)

## [0.2.3](https://github.com/ivov/lisette/compare/lisette-v0.2.2...lisette-v0.2.3) - 2026-05-11

- fix: detect fluent builders on promoted methods [#388](https://github.com/ivov/lisette/pull/388) [`50190aa`](https://github.com/ivov/lisette/commit/50190aa4a29c1dc45159d4a627296cfb4dacf646)
- ci: auto-publish vsix to marketplace and open-vsx registry [#387](https://github.com/ivov/lisette/pull/387) [`49ec00a`](https://github.com/ivov/lisette/commit/49ec00a326716b23900e38590bec2b43986c393c)
- refactor: improve diagnostics for keyword-named bindings [#386](https://github.com/ivov/lisette/pull/386) [`7dff8db`](https://github.com/ivov/lisette/commit/7dff8dbe8d88faa9793d18470876bf0674f75be8)
- fix: derive bindgen param names from types [#385](https://github.com/ivov/lisette/pull/385) [`40ff880`](https://github.com/ivov/lisette/commit/40ff880da2cb0756910699a72be6d71ed7f9ef0f)
- perf: parallelize module-graph parsing [#384](https://github.com/ivov/lisette/pull/384) [`89d0b74`](https://github.com/ivov/lisette/commit/89d0b74f70214f8f7b0990d3c38df69066146758)
- refactor: trim dead temps and discards from emitted Go [#383](https://github.com/ivov/lisette/pull/383) [`69179a9`](https://github.com/ivov/lisette/commit/69179a98a55b1438a51959d4e0d82c8e1fca1b74)
- perf: skip per-file gofmt during build [#381](https://github.com/ivov/lisette/pull/381) [`3f7a395`](https://github.com/ivov/lisette/commit/3f7a395a77c2dab18cda281997a873ec43eeb65f)
- fix: support Option<T> when T is a fn type alias [#379](https://github.com/ivov/lisette/pull/379) [`417a247`](https://github.com/ivov/lisette/commit/417a24717d77ad40ba504c7783bb188742217ab3)

## [0.2.2](https://github.com/ivov/lisette/compare/lisette-v0.2.1...lisette-v0.2.2) - 2026-05-10

- feat: third-party Go dependencies [#374](https://github.com/ivov/lisette/pull/374) [`5276b12`](https://github.com/ivov/lisette/commit/5276b12f3897f2a33bca1eab0dab2947d27a443a)
- perf: parallelize codegen [#376](https://github.com/ivov/lisette/pull/376) [`17085bb`](https://github.com/ivov/lisette/commit/17085bbefb8b18a03b3b193e9f87aa4196e3fe05)
- chore: update vscode extension publish instructions [#375](https://github.com/ivov/lisette/pull/375) [`e050f20`](https://github.com/ivov/lisette/commit/e050f20f6f42cf7b58b9b48b47dcf7527cfe2eb9)
- chore: update vscode extension to 0.2.0 [#373](https://github.com/ivov/lisette/pull/373) [`162d62b`](https://github.com/ivov/lisette/commit/162d62b3001e94cfbec0ce00323d342c7ea8d4fd)
- perf: lazy third-party typedefs [#372](https://github.com/ivov/lisette/pull/372) [`5b4a849`](https://github.com/ivov/lisette/commit/5b4a84938722002b4f3dacee6e6f7b8bf926a253)
- fix: mut param correctness [#371](https://github.com/ivov/lisette/pull/371) [`4bb4369`](https://github.com/ivov/lisette/commit/4bb4369cb927d78396b166f461b663b97aa6fa02)
- ci: speed up e2e smoke project [#368](https://github.com/ivov/lisette/pull/368) [`a44fe7e`](https://github.com/ivov/lisette/commit/a44fe7e0bb7c4e042d9f749b197c7ae89f9121c2)
- perf: parallelize post-inference checks [#366](https://github.com/ivov/lisette/pull/366) [`3172bf1`](https://github.com/ivov/lisette/commit/3172bf123b16a55689d49a3b03d9f2b0e227f813)
- refactor: structure post-inference passes by role [#365](https://github.com/ivov/lisette/pull/365) [`0c1f2f5`](https://github.com/ivov/lisette/commit/0c1f2f5f0a5a2ff2320453b031c59702e847bf43)
- perf: parallelize inference [#364](https://github.com/ivov/lisette/pull/364) [`62e6798`](https://github.com/ivov/lisette/commit/62e6798f1de61c351ef2d9734aaeed9d03b27a26)
- refactor: make inference borrow the store immutably [#363](https://github.com/ivov/lisette/pull/363) [`d12a0d0`](https://github.com/ivov/lisette/commit/d12a0d04f701cac8ae2848152cc348e1740b78c6)
- fix: honor allow_unused_value config on bindgen methods [#361](https://github.com/ivov/lisette/pull/361) [`3ae673c`](https://github.com/ivov/lisette/commit/3ae673cf80ff71ab1615e0862fff6762bb97bf9f)
- fix: skip foreign-typed const in bindgen enum detection [#360](https://github.com/ivov/lisette/pull/360) [`c6b19d0`](https://github.com/ivov/lisette/commit/c6b19d000c1cb42b50ead2860fcbfb7ba337e739)
- ci: cut redundancy and speed up PR checks [#357](https://github.com/ivov/lisette/pull/357) [`891a835`](https://github.com/ivov/lisette/commit/891a835949ee8cd5d0c917fb1acd932bc1e2c61c)
- refactor: extend fn-as-lambda diagnostic to expressions [#359](https://github.com/ivov/lisette/pull/359) [`208d034`](https://github.com/ivov/lisette/commit/208d0344bd44b410ae00513b8db1ccc301a411a8)
- fix: emit recursive fn-type aliases as opaque types [#358](https://github.com/ivov/lisette/pull/358) [`be7cc99`](https://github.com/ivov/lisette/commit/be7cc9948b56656fd99bc13f9e24f0decaa6fbd1)
- feat: add allow_unused_value bindgen override for fluent APIs [#356](https://github.com/ivov/lisette/pull/356) [`1f18bcf`](https://github.com/ivov/lisette/commit/1f18bcff52cb654e39f38a0924cc800640b55005)
- refactor: improve ref qualifier misuse diagnostic [#355](https://github.com/ivov/lisette/pull/355) [`ecb4245`](https://github.com/ivov/lisette/commit/ecb42458dd83001565e8ac24f634639aa28abdd2)
- fix: salvage bindgen aliases to internal-package types [#354](https://github.com/ivov/lisette/pull/354) [`76ce06e`](https://github.com/ivov/lisette/commit/76ce06e7a294b40b04b1e2af01bdfce423c3394e)
- fix: emit never-bodied lambda as func() into unknown arg [#353](https://github.com/ivov/lisette/pull/353) [`7e357ce`](https://github.com/ivov/lisette/commit/7e357ceaed320bb8e3803a2a70f528f1c307c5e7)
- refactor: improve function type mismatch diagnostic [#352](https://github.com/ivov/lisette/pull/352) [`d19b9f5`](https://github.com/ivov/lisette/commit/d19b9f583b31f6d8f5f18921ab88496f2d003060)
- fix: exempt tag-attributed fields from unused_struct_field lint [#351](https://github.com/ivov/lisette/pull/351) [`cc4a0ac`](https://github.com/ivov/lisette/commit/cc4a0acc5e854438c522c374f9ecbe5279aabb5a)
- chore: upgrade to go 1.25.10 [#350](https://github.com/ivov/lisette/pull/350) [`f8763e9`](https://github.com/ivov/lisette/commit/f8763e9b9145c890caf364782816e097532207df)
- fix: preserve generic arity on skipped opaque type placeholders [#349](https://github.com/ivov/lisette/pull/349) [`6adc0b8`](https://github.com/ivov/lisette/commit/6adc0b8e8ef082f167242e233eb12a2dc3fee7d5)
- fix: preserve bindgen docs on funcs returning a local type [#347](https://github.com/ivov/lisette/pull/347) [`29e953b`](https://github.com/ivov/lisette/commit/29e953b9852d9836d1b95c4745d07d3ff6bed302)
- fix: bridge Option<T> on scalar args to *scalar at callsites [#346](https://github.com/ivov/lisette/pull/346) [`321abdc`](https://github.com/ivov/lisette/commit/321abdc856ff9bef746cd29a35efd0d74a60868a)

## [0.2.1](https://github.com/ivov/lisette/compare/lisette-v0.2.0...lisette-v0.2.1) - 2026-05-07

- fix: snapshot bytes-loop receiver to survive body reassignment [#344](https://github.com/ivov/lisette/pull/344) [`1eda7c8`](https://github.com/ivov/lisette/commit/1eda7c83360ca712eabfe0ec081878ea647c5d94)
- fix: freshen rest binding in let-else or-pattern [#343](https://github.com/ivov/lisette/pull/343) [`f88c1ef`](https://github.com/ivov/lisette/commit/f88c1efc04ec425748004e2e955ad5dbae639f60)
- fix: freshen go-escaped names that clash with siblings [#342](https://github.com/ivov/lisette/pull/342) [`36b6de2`](https://github.com/ivov/lisette/commit/36b6de2eac3ed929c3acd02235cba8bb77928d52)
- fix: declare local const in go scope for let/const shadowing [#341](https://github.com/ivov/lisette/pull/341) [`f7d8afb`](https://github.com/ivov/lisette/commit/f7d8afb898060786ec8ee944baae19e379fa637e)
- fix: ignore string literal contents in emit text scans [#337](https://github.com/ivov/lisette/pull/337) [`eaa4e69`](https://github.com/ivov/lisette/commit/eaa4e696d7badb625eb5632d6c8f7ae5d697bda0)
- fix: assign bindings in irrefutable let-else or-patterns [#338](https://github.com/ivov/lisette/pull/338) [`6362b34`](https://github.com/ivov/lisette/commit/6362b344ef9a4b17245d4ecdea92c553819867fc)
- ci: pass missing expression to fix build [#340](https://github.com/ivov/lisette/pull/340) [`495a95b`](https://github.com/ivov/lisette/commit/495a95bf8fc73b735b2a9a7977b30abc6d93e7e9)
- ci: skip pr-specific checks on release-plz prs [#339](https://github.com/ivov/lisette/pull/339) [`1b0a0ec`](https://github.com/ivov/lisette/commit/1b0a0ec0508c14239f642885a06894a324cacc5a)
- fix: capture mutable reads before later sibling setup runs [#336](https://github.com/ivov/lisette/pull/336) [`ead1048`](https://github.com/ivov/lisette/commit/ead1048e55e08eb0bb9f8bf840912cf9d048e38b)
- docs: show project layout in lis new and lis learn help [#335](https://github.com/ivov/lisette/pull/335) [`e16d7cf`](https://github.com/ivov/lisette/commit/e16d7cfc1138350b28a35add04e1f5155351c36e)
- fix: sequence receiver before range value in substring and slice [#334](https://github.com/ivov/lisette/pull/334) [`4559d27`](https://github.com/ivov/lisette/commit/4559d27f0d433fd18b5ebf8cfa036bf0b0b60884)
- fix: dereference Ref<string> receiver in substring call [#333](https://github.com/ivov/lisette/pull/333) [`855ae00`](https://github.com/ivov/lisette/commit/855ae00886ddb88b543b8ca5de16ef717a0041d3)
- fix: preserve & on UFCS composite literal receivers [#331](https://github.com/ivov/lisette/pull/331) [`589a9fa`](https://github.com/ivov/lisette/commit/589a9fad527778f17e89de446876406828677ecf)
- fix: parenthesize pointer newtype cast in match patterns [#332](https://github.com/ivov/lisette/pull/332) [`4035606`](https://github.com/ivov/lisette/commit/403560667e43c4b7d74ac311114ea81d6e790133)
- fix: camel-case snake_case names in receiver-method UFCS calls [#330](https://github.com/ivov/lisette/pull/330) [`06d098e`](https://github.com/ivov/lisette/commit/06d098e148ffa23642b185f8d9b3799666f62207)
- fix: export camel-case method names in interface adapters [#329](https://github.com/ivov/lisette/pull/329) [`d35f8d3`](https://github.com/ivov/lisette/commit/d35f8d3113ff9c9089f3f90719e206a669be0a51)
- fix: handle escaped quotes in fmt.Println f-string collapse [#328](https://github.com/ivov/lisette/pull/328) [`a798d21`](https://github.com/ivov/lisette/commit/a798d218279b08e0d201fc5e05a830bf9dc9590e)
- fix: preserve short-circuit when && or || RHS needs setup [#326](https://github.com/ivov/lisette/pull/326) [`4126b50`](https://github.com/ivov/lisette/commit/4126b50af7cd378fdc0730fe3ca8ef23e0a7ff75)

## [0.2.0](https://github.com/ivov/lisette/compare/lisette-v0.1.26...lisette-v0.2.0) - 2026-05-06

- refactor!: reject positional access on string [#323](https://github.com/ivov/lisette/pull/323) [`a7c7245`](https://github.com/ivov/lisette/commit/a7c7245aab153ad88d44ef5ea93529c26cad2d8c)
- feat: add substring, bytes, and runes methods on string [#320](https://github.com/ivov/lisette/pull/320) [`ebd679d`](https://github.com/ivov/lisette/commit/ebd679deed12383da5cbeffc9b60b0b3770c3104)
- refactor!: switch variadic spread to postfix syntax [#322](https://github.com/ivov/lisette/pull/322) [`8f597b2`](https://github.com/ivov/lisette/commit/8f597b2ae00b68a6cf48df63077d66e382496865)
- fix: allow rune to string cast, block rune to byte cast [#318](https://github.com/ivov/lisette/pull/318) [`3ce2fb6`](https://github.com/ivov/lisette/commit/3ce2fb6b8c8859ee10ef9508b093fff1bc62cb3c)
- docs: add snippet sharing to playground [#319](https://github.com/ivov/lisette/pull/319) [`0234b42`](https://github.com/ivov/lisette/commit/0234b42056f50951226d60d76735bc33945886a0)
- docs: add link to WASM playground in index.html [#310](https://github.com/ivov/lisette/pull/310) [`38d46b6`](https://github.com/ivov/lisette/commit/38d46b648b9678b94fe4a4bef7189648b985d02b)
- refactor: add rune-indexed string access to prelude [#317](https://github.com/ivov/lisette/pull/317) [`5395a8d`](https://github.com/ivov/lisette/commit/5395a8db2fbd2e85826cfbe11e5ea3c168b9449d)
- refactor: move third-party typedefs into project [#315](https://github.com/ivov/lisette/pull/315) [`d7d8adc`](https://github.com/ivov/lisette/commit/d7d8adcfaf5c87e109ba3bc135e49876c9793d53)
- refactor: thread target through third-party typedef cache paths [#314](https://github.com/ivov/lisette/pull/314) [`e3d6ed2`](https://github.com/ivov/lisette/commit/e3d6ed2bb8324144a188de7baf55846dd1addf16)
- ci: lint bindgen and prelude with golangci-lint [#313](https://github.com/ivov/lisette/pull/313) [`48e57b8`](https://github.com/ivov/lisette/commit/48e57b8eab6d034242173b5d1651fb640db11b46)
- fix: disambiguate bindgen aliases on shared trailing segments [#312](https://github.com/ivov/lisette/pull/312) [`cc2f820`](https://github.com/ivov/lisette/commit/cc2f820039f6c8dd0551dbc27b922eb91eef1295)
- fix: suggest `..` zero-fill in missing-fields help [#311](https://github.com/ivov/lisette/pull/311) [`219cf0a`](https://github.com/ivov/lisette/commit/219cf0ab8f5f062320eabbf6fd4d5e195d328dc2)
- fix: preserve all returns when trailing bool is a flag [#307](https://github.com/ivov/lisette/pull/307) [`227e61e`](https://github.com/ivov/lisette/commit/227e61e7e23246ee08e69e671d2df53be5708c17)

## [0.1.26](https://github.com/ivov/lisette/compare/lisette-v0.1.25...lisette-v0.1.26) - 2026-05-04

- fix: tailor nil and member-not-found help to context [#306](https://github.com/ivov/lisette/pull/306) [`aa12647`](https://github.com/ivov/lisette/commit/aa12647673d779dad69b6142b3b61b681611a205)
- fix: preserve byte and rune aliases in bindgen output [#305](https://github.com/ivov/lisette/pull/305) [`90601ad`](https://github.com/ivov/lisette/commit/90601ad0f2d472e7177285f3ad024d0198d2354c)
- fix: fixes to lsp and formatting [#293](https://github.com/ivov/lisette/pull/293) [`d50732f`](https://github.com/ivov/lisette/commit/d50732fd634225042d2f12b9d55aca61d03ab0e2)
- fix: hint parent module when lis add hits a sub-package [#304](https://github.com/ivov/lisette/pull/304) [`60f3f10`](https://github.com/ivov/lisette/commit/60f3f10c9078db67820781db94711a65e24ee3d6)
- fix: tailor diagnostics for `Self` in impl and `mut` struct fields [#303](https://github.com/ivov/lisette/pull/303) [`ce0869f`](https://github.com/ivov/lisette/commit/ce0869fe20c4997a8cd452d887ee76e7484161e4)
- fix: alias empty named interfaces to Unknown [#302](https://github.com/ivov/lisette/pull/302) [`34a48a8`](https://github.com/ivov/lisette/commit/34a48a81f9e657f2296cbdc832773857d6f912db)
- fix: bind unexported error sentinels as error [#301](https://github.com/ivov/lisette/pull/301) [`b0fb981`](https://github.com/ivov/lisette/commit/b0fb98134364f14b42f04158341204b96f703e40)
- fix: suppress duplicate type mismatch on `?` in non-carrier function [#300](https://github.com/ivov/lisette/pull/300) [`cb4545d`](https://github.com/ivov/lisette/commit/cb4545dff1816f808e7a7fcede4873315b2edf37)
- fix: align select arm type inference with match [#297](https://github.com/ivov/lisette/pull/297) [`2603c6f`](https://github.com/ivov/lisette/commit/2603c6f23f38468da6e5b166df34e6355222c110)
- fix: lead native_method_value help with call-direct form [#296](https://github.com/ivov/lisette/pull/296) [`192cdd6`](https://github.com/ivov/lisette/commit/192cdd6868b7d0adba9c668ed9440c1dbdd35c2d)
- fix: reach pointer-receiver methods on package-var struct fields [#295](https://github.com/ivov/lisette/pull/295) [`21bc10f`](https://github.com/ivov/lisette/commit/21bc10fa0b2b2195b80738ea1d24dd2ed3ef3ab1)
- fix: redirect unwrap/expect diagnostic on Option/Result/Partial [#294](https://github.com/ivov/lisette/pull/294) [`9b99b08`](https://github.com/ivov/lisette/commit/9b99b08ae61f666d8437e9eedad6b454dc83f118)
- fix: ship per-target stdlib typedefs [#291](https://github.com/ivov/lisette/pull/291) [`8f635cc`](https://github.com/ivov/lisette/commit/8f635cceae09fb0bf47100b46c0adfa9b3276b90)
- ci: fix pr title trigger [#292](https://github.com/ivov/lisette/pull/292) [`40e55b3`](https://github.com/ivov/lisette/commit/40e55b32fa028597a3427b78340b0497153bf40c)
- refactor: consolidate checker definitions and walkers [#290](https://github.com/ivov/lisette/pull/290) [`c895586`](https://github.com/ivov/lisette/commit/c8955865b2c1cf1330f3229ef85edff045f4a0fa)
- refactor: thread target argument through stdlib typedef lookups [#289](https://github.com/ivov/lisette/pull/289) [`0b7570c`](https://github.com/ivov/lisette/commit/0b7570c7a767ff1b577adc67dc984277b279d9c5)
- refactor: consolidate checker cursor and scope setup [#287](https://github.com/ivov/lisette/pull/287) [`ce653c9`](https://github.com/ivov/lisette/commit/ce653c9bf32e618458ba737f3d56bc29e6a6439a)

## [0.1.25](https://github.com/ivov/lisette/compare/lisette-v0.1.24...lisette-v0.1.25) - 2026-05-03

- fix: propagate expected type through generic positions [#286](https://github.com/ivov/lisette/pull/286) [`9777eec`](https://github.com/ivov/lisette/commit/9777eec86c5fc3d3b636f6820f5b7cadcc561d8c)
- chore: parameterize stdlib typedef regen target [#285](https://github.com/ivov/lisette/pull/285) [`6eccca3`](https://github.com/ivov/lisette/commit/6eccca3954ce7597618ba7aa715ee1113a11e766)
- fix: bridge option<T> backed by go *T across reads and writes [#284](https://github.com/ivov/lisette/pull/284) [`cebbe84`](https://github.com/ivov/lisette/commit/cebbe84d52d4096d58061fae5b73b2f22f46867e)
- refactor: drop stale Nominal type uses in lsp [#283](https://github.com/ivov/lisette/pull/283) [`2dc196c`](https://github.com/ivov/lisette/commit/2dc196cb1baca598173aad871f6ef24d1b0f9f67)
- fix: show lsp completions through ref types [#282](https://github.com/ivov/lisette/pull/282) [`0b39743`](https://github.com/ivov/lisette/commit/0b39743312f9f4c67aa0345f5659c8fe1fad547b)
- fix: skip subpackage check for multi-module monorepo siblings [#280](https://github.com/ivov/lisette/pull/280) [`0a270b7`](https://github.com/ivov/lisette/commit/0a270b72c2ecbac4accc1d90d3751fcc8c02aa54)
- fix: lower paired Go errors as tuple of Option<error> [#279](https://github.com/ivov/lisette/pull/279) [`1393aaa`](https://github.com/ivov/lisette/commit/1393aaabf77b6309b6568c37bf443811530b1994)
- fix: tolerate transitive bindgen failures in lis add [#278](https://github.com/ivov/lisette/pull/278) [`fcad1a0`](https://github.com/ivov/lisette/commit/fcad1a0fdf8be16efe2a863df0a18ec8734f10b7)
- ci: pin bindgen target platform [#276](https://github.com/ivov/lisette/pull/276) [`62f22ea`](https://github.com/ivov/lisette/commit/62f22eabc46ea897d785f5ca55ce406cb2f10066)
- fix: inherit invocation cwd in lis run [#275](https://github.com/ivov/lisette/pull/275) [`fbe12d2`](https://github.com/ivov/lisette/commit/fbe12d25b1e32a175ffa80202ab4323b6d2e4d12)
- ci: detect stdlib typedef drift [#274](https://github.com/ivov/lisette/pull/274) [`67dbb9b`](https://github.com/ivov/lisette/commit/67dbb9bdafc51b2edd0c1f741ab8ea3979bc773d)
- feat: add nilable_param bindgen override [#273](https://github.com/ivov/lisette/pull/273) [`b54a42d`](https://github.com/ivov/lisette/commit/b54a42df5e6968e8392639aae9d7da221260a74f)
- chore: strip blank line above skipped-field comments in typedefs [#272](https://github.com/ivov/lisette/pull/272) [`81b4497`](https://github.com/ivov/lisette/commit/81b4497818d8a4fd1b104ed411446967df20ec0e)
- fix: hint unwrap for member access on Option/Result [#271](https://github.com/ivov/lisette/pull/271) [`cb8c6c0`](https://github.com/ivov/lisette/commit/cb8c6c0ee68a7c3cd028cd9739afe52a3d3dee51)
- feat: lift ban on userland unknown [#270](https://github.com/ivov/lisette/pull/270) [`dc69354`](https://github.com/ivov/lisette/commit/dc693542dcfa70ca75a44ff704f935c5862902e6)
- fix: suppress cascading diagnostics for error type [#269](https://github.com/ivov/lisette/pull/269) [`7824122`](https://github.com/ivov/lisette/commit/7824122e0c7bd86d306618b7dbdc331ccf60fca7)
- docs: refresh agents.md template [#268](https://github.com/ivov/lisette/pull/268) [`9e20b00`](https://github.com/ivov/lisette/commit/9e20b00f013cc9f5fde105e94402de181171ed75)
- fix: hint as-cast for numeric type mismatches [#267](https://github.com/ivov/lisette/pull/267) [`db3e352`](https://github.com/ivov/lisette/commit/db3e35225846952de3af0785276c40773306737a)
- ci: drop changelog tripwire from release gates [#266](https://github.com/ivov/lisette/pull/266) [`51d3ec3`](https://github.com/ivov/lisette/commit/51d3ec3cc430ffa31dbbf60ce16d64b639e2ebf7)
- feat: revamp CLI surface [#265](https://github.com/ivov/lisette/pull/265) [`100a91b`](https://github.com/ivov/lisette/commit/100a91b5ce81faca24ebb1813155e6b0a5446fe2)
- fix: improve diagnostic for backticks outside attribute position [#264](https://github.com/ivov/lisette/pull/264) [`569e309`](https://github.com/ivov/lisette/commit/569e309926be460265508e2b881fa5956fcf016d)
- feat: add beginner-friendly diagnostics [#263](https://github.com/ivov/lisette/pull/263) [`063a155`](https://github.com/ivov/lisette/commit/063a1550301372100814cdc416f74bd40cef26e9)
- fix: stop double-wrapping callbacks bound to Go fn aliases [#261](https://github.com/ivov/lisette/pull/261) [`6b8894c`](https://github.com/ivov/lisette/commit/6b8894cb0674008e8c9feba41822c56154a22c96)

## [0.1.24](https://github.com/ivov/lisette/compare/lisette-v0.1.23...lisette-v0.1.24) - 2026-05-01

- refactor: clean up diagnostics taxonomy [#260](https://github.com/ivov/lisette/pull/260) [`608b213`](https://github.com/ivov/lisette/commit/608b2136b2ad799405c86fb3267b13b85b56f2f7)
- ci: skip release gates on release-plz tmp branches [#259](https://github.com/ivov/lisette/pull/259) [`23678d4`](https://github.com/ivov/lisette/commit/23678d47a306c957c225fba745f3b513485cf8c5)
- test: refresh snapshots [#258](https://github.com/ivov/lisette/pull/258) [`57060a4`](https://github.com/ivov/lisette/commit/57060a413d59994c0a58e32a32d1dc1fa2df1fcb)
- fix: recognize alias of interface constraints in bindgen [#256](https://github.com/ivov/lisette/pull/256) [`4b897b2`](https://github.com/ivov/lisette/commit/4b897b2965d9e73001708108130d3429c3b36f33)
- fix: skip generated Go files when pruning orphans [#255](https://github.com/ivov/lisette/pull/255) [`fee93e5`](https://github.com/ivov/lisette/commit/fee93e53c7f20a75c2a6113699715cac9d92403e)
- fix: report tail-position return-value mismatches [#254](https://github.com/ivov/lisette/pull/254) [`47ada42`](https://github.com/ivov/lisette/commit/47ada42712c709f30d1021184c541e486afbdf99)
- test: make discarded value tails explicit in lint fixtures [#253](https://github.com/ivov/lisette/pull/253) [`d6cec38`](https://github.com/ivov/lisette/commit/d6cec38ca318c23778ee5e055debf504bd847127)
- fix: preserve substitution through generic alias peeling [#250](https://github.com/ivov/lisette/pull/250) [`3ad1ed2`](https://github.com/ivov/lisette/commit/3ad1ed23cb02d8459e58416aca162dbf422fd5ca)
- fix: preserve comment placement in formatter [#247](https://github.com/ivov/lisette/pull/247) [`d2b4ca2`](https://github.com/ivov/lisette/commit/d2b4ca2ee109ac71a2f07fb467594236d07ced44)
- chore: rebuild playground against ab8933d [#249](https://github.com/ivov/lisette/pull/249) [`c28b2a2`](https://github.com/ivov/lisette/commit/c28b2a24e7781c142d19a7225fd698f8c90e7ee2)
- ci: parallelize checks and reorganize e2e tests [#248](https://github.com/ivov/lisette/pull/248) [`ab8933d`](https://github.com/ivov/lisette/commit/ab8933d6431c391bda59dcfeb4539130eaa73180)
- ci: skip regen step when release-plz returns no PR [#246](https://github.com/ivov/lisette/pull/246) [`a11466e`](https://github.com/ivov/lisette/commit/a11466ebe80507608852c5cd225e79a97dc7655f)
- ci: fix release-publish iteration on workspace crates [#245](https://github.com/ivov/lisette/pull/245) [`3a3ab02`](https://github.com/ivov/lisette/commit/3a3ab0215942b344a7bfc4882c74db74dd4f8d1a)

## [0.1.23](https://github.com/ivov/lisette/compare/lisette-v0.1.22...lisette-v0.1.23) - 2026-04-30

- fix: lsp completion for built-in prelude types [#240](https://github.com/ivov/lisette/pull/240) [`abe54cb`](https://github.com/ivov/lisette/commit/abe54cbd76151c65776cb86bb5e6601a7114cf0c)
- fix: skip arrays in bindgen [#244](https://github.com/ivov/lisette/pull/244) [`7f9ce5c`](https://github.com/ivov/lisette/commit/7f9ce5c105a34dbabebe4bd521900938f0193ff8)
- fix: keep struct-field comments on their own line [#243](https://github.com/ivov/lisette/pull/243) [`8add90c`](https://github.com/ivov/lisette/commit/8add90cbdacff26c82b60ab54329f3518c83a4de)
- fix: preserve underscores in go-imported member names [#242](https://github.com/ivov/lisette/pull/242) [`d10f4ef`](https://github.com/ivov/lisette/commit/d10f4ef6ae8e05addc74b536a40ce833c159f18c)
- perf: run bindgen once per module [#241](https://github.com/ivov/lisette/pull/241) [`130908d`](https://github.com/ivov/lisette/commit/130908d0eed93c3a12a78b2650d6a34ff82a93fd)
- fix: preserve comments between method chain segments [#234](https://github.com/ivov/lisette/pull/234) [`ed4e502`](https://github.com/ivov/lisette/commit/ed4e502f136bd540c36d2ec5706fffa54ecc2f7b)
- fix: name unhinted type vars without panicking [#239](https://github.com/ivov/lisette/pull/239) [`4153114`](https://github.com/ivov/lisette/commit/4153114477b107932c1f27dfe67af1b9633abc1f)
- ci: bot-author regen and restore upcoming heading [#237](https://github.com/ivov/lisette/pull/237) [`323e2b4`](https://github.com/ivov/lisette/commit/323e2b4b60754a2219eb807bc84453a6a99dddcf)
- ci: include off-crate commits in release changelog [#235](https://github.com/ivov/lisette/pull/235) [`804e691`](https://github.com/ivov/lisette/commit/804e691359fcbb1174973b0f54b73fe60e940d48)
- fix: emit pub on exported go constants in typedefs [#231](https://github.com/ivov/lisette/pull/231) [`8fec935`](https://github.com/ivov/lisette/commit/8fec93520915dfacf9f91d2d21f92c73cbea27c9)
- fix: detect colliding go imports at emit time [#230](https://github.com/ivov/lisette/pull/230) [`e13c215`](https://github.com/ivov/lisette/commit/e13c2158a5b9f44d3c9f1458b6cf54e42e2fa024)
- fix: render unreachable unexported go types as unknown [#229](https://github.com/ivov/lisette/pull/229) [`c3bd761`](https://github.com/ivov/lisette/commit/c3bd761ef54e1262cdad0b79387a3eddca3f5060)
- fix: reject unknown vs concrete inside invariant generic positions [#228](https://github.com/ivov/lisette/pull/228) [`91bddaa`](https://github.com/ivov/lisette/commit/91bddaa055290a754fbf565c39a787ad119668ab)
- fix: pascalcase exported go names [#227](https://github.com/ivov/lisette/pull/227) [`d91785e`](https://github.com/ivov/lisette/commit/d91785eb0d0aee9a0ee49914b2b11dcea20c5e06)
- fix: allow assignment to pub var through import alias [#225](https://github.com/ivov/lisette/pull/225) [`2df0d3e`](https://github.com/ivov/lisette/commit/2df0d3ece32e17bd57321c507429d93145cc981a)
- feat: recognize named-interface bounds in bindgen generics [#224](https://github.com/ivov/lisette/pull/224) [`6a41b27`](https://github.com/ivov/lisette/commit/6a41b272cf3ff55d777dc2a4ac87cd68a888d407)
- ci: skip autogenerated release.yml in ratchet recipes [#222](https://github.com/ivov/lisette/pull/222) [`3e203c9`](https://github.com/ivov/lisette/commit/3e203c9e6600eb9179f130cb72502bb66eeba898)

## [0.1.22](https://github.com/ivov/lisette/compare/lisette-v0.1.21...lisette-v0.1.22) - 2026-04-28

- ci: unpin release.yml so cargo-dist can regenerate it [#221](https://github.com/ivov/lisette/pull/221) [`b81058f`](https://github.com/ivov/lisette/commit/b81058f85509d5e12ded66085af309464851e45e)
- feat: multi-line string literals [#208](https://github.com/ivov/lisette/pull/208) [`05127d9`](https://github.com/ivov/lisette/commit/05127d9af9a29c60eadde2b9856ec3cce6bd1179)
- ci: pin workflow actions to immutable SHAs [#220](https://github.com/ivov/lisette/pull/220) [`30c05ea`](https://github.com/ivov/lisette/commit/30c05ead8613c3435c79da9bc27023adc8b17650)
- feat: lift interface{} params to Ref<T> for reflection decoders [#219](https://github.com/ivov/lisette/pull/219) [`6f7b04a`](https://github.com/ivov/lisette/commit/6f7b04a4b6e0f00145c011dbe05609f5893fd67f)
- feat: silence unused_value on fluent-builder method returns [#218](https://github.com/ivov/lisette/pull/218) [`1266fde`](https://github.com/ivov/lisette/commit/1266fdebdfc8dc3fff6cbd0ae4de570c167852d7)
- fix: alias single-segment paths colliding with longer external paths [#217](https://github.com/ivov/lisette/pull/217) [`efd89d3`](https://github.com/ivov/lisette/commit/efd89d330115c0a3dbab95b8c30562d74f7973b9)
- feat: do not require trailing () in unit-context lambdas [#216](https://github.com/ivov/lisette/pull/216) [`49e44be`](https://github.com/ivov/lisette/commit/49e44be0bfc47578dd63a803694bb4a5dfa30fbb)
- feat: suggest go prefix for unprefixed declared go deps [#215](https://github.com/ivov/lisette/pull/215) [`490709e`](https://github.com/ivov/lisette/commit/490709ef90048cc525c014bea04e20a4048bd744)
- fix: alias bindgen imports that collide on package name [#212](https://github.com/ivov/lisette/pull/212) [`fbf4f74`](https://github.com/ivov/lisette/commit/fbf4f746c9b0e45f78bd362f8674b69b01a1ce8b)
- fix: bind variadic interface methods to VarArgs [#211](https://github.com/ivov/lisette/pull/211) [`500865b`](https://github.com/ivov/lisette/commit/500865be30ad6b3c33a16c66bcd3b965c7a30375)
- fix: avoid cloning lhs on compound assign with invalid target [#209](https://github.com/ivov/lisette/pull/209) [`e481af2`](https://github.com/ivov/lisette/commit/e481af2e5562e96cbb788b696aec5b600407eb30)
- feat: zero-fill spread [#210](https://github.com/ivov/lisette/pull/210) [`9681887`](https://github.com/ivov/lisette/commit/9681887c997b4c3d6e63e4107d883294edf3e679)
- feat: support sql.Scanner and driver.Valuer on option [#206](https://github.com/ivov/lisette/pull/206) [`6091db8`](https://github.com/ivov/lisette/commit/6091db8e03b00bb0d765f52586a483c9da29a8de)

## [0.1.21](https://github.com/ivov/lisette/compare/lisette-v0.1.20...lisette-v0.1.21) - 2026-04-26

- fix: alias transparency with generics [#203](https://github.com/ivov/lisette/pull/203) [`5b10e59`](https://github.com/ivov/lisette/commit/5b10e5927827738666e048d0f7a4c23a156b149e)
- fix: option-wrap nilable go fn-typed struct fields [#201](https://github.com/ivov/lisette/pull/201) [`1ce66e0`](https://github.com/ivov/lisette/commit/1ce66e039a599565fe95daedaba2e6dc4395d16f)
- feat: add min and max prelude builtins [#200](https://github.com/ivov/lisette/pull/200) [`4c3d8d5`](https://github.com/ivov/lisette/commit/4c3d8d5125f3ac875f685ab7feab93166099980f)
- feat: forbid shadowing prelude functions [#199](https://github.com/ivov/lisette/pull/199) [`f908124`](https://github.com/ivov/lisette/commit/f9081241ca2fdff0ec0337820aa59090295ba633)
- feat: recognize M ~map[K]V as type-parameter shape [#198](https://github.com/ivov/lisette/pull/198) [`b22621e`](https://github.com/ivov/lisette/commit/b22621e2e44b9d7b661e624f2c9337a3d387016e)
- feat: support Comparable and cmp.Ordered as type-parameter bounds [#197](https://github.com/ivov/lisette/pull/197) [`e762d8c`](https://github.com/ivov/lisette/commit/e762d8c659f610b1f947b58c775037daf29e564a)
- fix: emit address-of for option fields at go struct literals [#196](https://github.com/ivov/lisette/pull/196) [`8958143`](https://github.com/ivov/lisette/commit/8958143ac567c75d14fa17eb4104a576a1b81e79)
- fix: convert fn values to match prelude callback abi [#194](https://github.com/ivov/lisette/pull/194) [`18d093b`](https://github.com/ivov/lisette/commit/18d093bf81c7503dc4aebd819d80e1e381257e21)
- fix: preserve fn-alias type on let-bound lambdas [#192](https://github.com/ivov/lisette/pull/192) [`a931695`](https://github.com/ivov/lisette/commit/a93169581c6611c4674f4ef05334d9b7ea02f8fc)
- feat: reject String/GoString impl methods with wrong signature [#191](https://github.com/ivov/lisette/pull/191) [`4097a1b`](https://github.com/ivov/lisette/commit/4097a1b8cd6fad1799edd0aaec6af83824aeaff7)
- fix: avoid unused go bindings in select-recv and while-let [#189](https://github.com/ivov/lisette/pull/189) [`7be8550`](https://github.com/ivov/lisette/commit/7be8550fda99cb3b75227aa331c64b41b058459e)
- fix: use %p for func-typed fields in auto-stringers [#188](https://github.com/ivov/lisette/pull/188) [`6331009`](https://github.com/ivov/lisette/commit/633100914497636b78e55ee57f76c6f0a6fc11cd)
- fix: do not lower-classify prelude-fn callees [#186](https://github.com/ivov/lisette/pull/186) [`8ad9926`](https://github.com/ivov/lisette/commit/8ad992603b33817b773eae9221ab2ab94a6e428f)
- ci: add emit-runtime suite and restructure release gates [#185](https://github.com/ivov/lisette/pull/185) [`02f6e09`](https://github.com/ivov/lisette/commit/02f6e0951415d370cadfedd0d87cde2a81fb4de4)

## [0.1.20](https://github.com/ivov/lisette/compare/lisette-v0.1.19...lisette-v0.1.20) - 2026-04-25

- feat: add sentinel-int hint and lower any nilable err type [`c11e1de`](https://github.com/ivov/lisette/commit/c11e1de139756c1a324e9dd345a4bc05c6e6ca12)
- refactor: lower wrapping types to go-native abi at function boundaries [#184](https://github.com/ivov/lisette/pull/184) [`541e21d`](https://github.com/ivov/lisette/commit/541e21dfd9d15cb7c50dbbe8fe72e19efc4dc205)
- feat: add lis sync to reconcile manifest with source [#183](https://github.com/ivov/lisette/pull/183) [`3d694a8`](https://github.com/ivov/lisette/commit/3d694a844817a96e53bdccc273d88d26cfe40000)
- fix: normalize string escapes when comparing patterns [#182](https://github.com/ivov/lisette/pull/182) [`22b0157`](https://github.com/ivov/lisette/commit/22b015769cd4fe1ab068b40624462adf295502ad)
- docs: surface lis lsp in help and quickstart [#181](https://github.com/ivov/lisette/pull/181) [`61da796`](https://github.com/ivov/lisette/commit/61da796c876ff1b9dfacd07229f7061d638099bd)
- feat: introduce raw string literals [#179](https://github.com/ivov/lisette/pull/179) [`4dcd1cb`](https://github.com/ivov/lisette/commit/4dcd1cbefbefb786ea4d8342c25a7d5802adbd2e)
- fix: harden go interface dispatch for user impl methods [#175](https://github.com/ivov/lisette/pull/175) [`9194ef5`](https://github.com/ivov/lisette/commit/9194ef52825ca3b47a02bf1bba8e501c666e5e1a)
- docs: add prebuilt install path [#174](https://github.com/ivov/lisette/pull/174) [`65fd842`](https://github.com/ivov/lisette/commit/65fd8421d7b139346b5ed6ba96fe8014afa9374f)

## [0.1.19](https://github.com/ivov/lisette/compare/lisette-v0.1.18...lisette-v0.1.19) - 2026-04-24

- fix: place enum constructors beside their enum definition [#172](https://github.com/ivov/lisette/pull/172) [`e367406`](https://github.com/ivov/lisette/commit/e3674063d070d130d53be9b43525d4a7fcd41b86)
- fix: suppress auto-stringer when user method uses Go casing [#171](https://github.com/ivov/lisette/pull/171) [`6c4bb49`](https://github.com/ivov/lisette/commit/6c4bb490bd5402b82677743e8dff28d45e0af5cc)
- refactor: prep parallel semantics [#170](https://github.com/ivov/lisette/pull/170) [`54a2a0c`](https://github.com/ivov/lisette/commit/54a2a0cf07d34f1f9bea6205fb963114351790a1)
- ci: ship prebuilt binaries [#165](https://github.com/ivov/lisette/pull/165) [`a11eda1`](https://github.com/ivov/lisette/commit/a11eda1696aa2e0c9b3e7cc311d8031125a17529)
- ci: rename release.yml to release-plz.yml [`92c3ba7`](https://github.com/ivov/lisette/commit/92c3ba7f4da9b90f66045cccea47d0bf73bc1167)

## [0.1.18](https://github.com/ivov/lisette/compare/lisette-v0.1.17...lisette-v0.1.18) - 2026-04-23

- fix: bolster misuse diagnostics [#164](https://github.com/ivov/lisette/pull/164) [`11c86eb`](https://github.com/ivov/lisette/commit/11c86eb1c0c4b3f6d8189bf5eb147fafcfd3f51f)
- fix: align const semantics with Go [#162](https://github.com/ivov/lisette/pull/162) [`db32264`](https://github.com/ivov/lisette/commit/db32264e14e1bb9748c5c597192abe316fb4e741)
- refactor: overhaul type representation and inference state [#161](https://github.com/ivov/lisette/pull/161) [`8468519`](https://github.com/ivov/lisette/commit/84685195a0e777ae01835d68969eb11c69516a6a)
- refactor: consolidate emit coercions and decision walkers [#157](https://github.com/ivov/lisette/pull/157) [`ed1cf48`](https://github.com/ivov/lisette/commit/ed1cf48f37b7f8c33f79bc660c636306d1fea27c)

## [0.1.17](https://github.com/ivov/lisette/compare/lisette-v0.1.16...lisette-v0.1.17) - 2026-04-21

- fix: auto-address struct literal receivers for ref methods [#156](https://github.com/ivov/lisette/pull/156) [`4f4f065`](https://github.com/ivov/lisette/commit/4f4f065484795310e901c29c6b47eb45a503d2a3)
- fix: always break multi-step pipelines [#147](https://github.com/ivov/lisette/pull/147) [`fbcf877`](https://github.com/ivov/lisette/commit/fbcf877c419749bcec9ba85822ae7e3d8a4af0e5)
- fix: omit match label when all guarded arms diverge [#155](https://github.com/ivov/lisette/pull/155) [`0b61fd6`](https://github.com/ivov/lisette/commit/0b61fd6dff511fe7d1bcdc60c3cb8e5cee40c417)
- fix: reset emit scope between impl methods to prevent name leak [#154](https://github.com/ivov/lisette/pull/154) [`259a32c`](https://github.com/ivov/lisette/commit/259a32c9498c4323f1259ffbcd4fd1fe2b165488)
- fix: reject bare record struct names used as values [#153](https://github.com/ivov/lisette/pull/153) [`a057965`](https://github.com/ivov/lisette/commit/a0579651d6ed5219b4f5b1d83cc55fd146aec978)
- refactor: simplify emit layer readability and structure [#151](https://github.com/ivov/lisette/pull/151) [`4dc768e`](https://github.com/ivov/lisette/commit/4dc768ef45a196f1d9f532f95856d15b0e7f582f)

## [0.1.16](https://github.com/ivov/lisette/compare/lisette-v0.1.15...lisette-v0.1.16) - 2026-04-20

- feat: as pattern bindings [#145](https://github.com/ivov/lisette/pull/145) [`4688fd6`](https://github.com/ivov/lisette/commit/4688fd67f6a774fa4857a088a90825f70b8175ae)
- fix: guard else-strip against duplicate var declarations [`6a73446`](https://github.com/ivov/lisette/commit/6a7344659e06543b67f1418c3eb377fa97302b3b)
- refactor: flatten guard else in type switch case bodies [`2351ae5`](https://github.com/ivov/lisette/commit/2351ae55fac9f85e78156721506109d5e1d43994)
- fix: avoid duplicate var declarations in interface match arms [`c601bf9`](https://github.com/ivov/lisette/commit/c601bf9b341ddb0203c6683cbb50ee7be34d0da5)
- fix: emit explicit guard failure in type switch chain case bodies [`199cdbc`](https://github.com/ivov/lisette/commit/199cdbcf3120628eddc3c8151a70a0a81eefee9d)
- fix: emit type switch for or-pattern on interface with field checks [`d8352ca`](https://github.com/ivov/lisette/commit/d8352ca9d2a060a474205c2e2776d43166069c62)
- fix: emit Go type switch case for or-pattern on interface [#143](https://github.com/ivov/lisette/pull/143) [`478d1bd`](https://github.com/ivov/lisette/commit/478d1bdb0417c37fab0cbc25535360e13ebc66dc)
- fix: emit type switch when matching on aliased go interface [#142](https://github.com/ivov/lisette/pull/142) [`97f7f5a`](https://github.com/ivov/lisette/commit/97f7f5a83f6118c33e01d4853947a2f6f3daaa16)

## [0.1.15](https://github.com/ivov/lisette/compare/lisette-v0.1.14...lisette-v0.1.15) - 2026-04-19

- fix: emit Go type switch when matching on an interface type [#138](https://github.com/ivov/lisette/pull/138) [`9803025`](https://github.com/ivov/lisette/commit/9803025475bfd7efb70e91176784887a8387d023)

## [0.1.14](https://github.com/ivov/lisette/compare/lisette-v0.1.13...lisette-v0.1.14) - 2026-04-19

- fix: embed external config in bindgen binary as default [#136](https://github.com/ivov/lisette/pull/136) [`ef30543`](https://github.com/ivov/lisette/commit/ef305432fc7893da03cbf4e480263db3fd0f08c2)
- fix: unit-body lambda emits nil [#135](https://github.com/ivov/lisette/pull/135) [`ceb7c21`](https://github.com/ivov/lisette/commit/ceb7c2137d21cdf5d53373567381bed1b420d651)
- fix: keep transitive go imports whose package name differs from path [#134](https://github.com/ivov/lisette/pull/134) [`9843014`](https://github.com/ivov/lisette/commit/9843014edb72cbf47a6f8f80b5e9561e8873cee6)
- fix: peel type aliases in interface and field checks [#133](https://github.com/ivov/lisette/pull/133) [`3245d95`](https://github.com/ivov/lisette/commit/3245d95be8b8a5e2985b5d1d02c406deab847db9)
- feat: add ..expr spread argument syntax for variadic calls [#124](https://github.com/ivov/lisette/pull/124) [`8348b5d`](https://github.com/ivov/lisette/commit/8348b5dab0dc8b2685271796beacc8dafa899a71)
- feat: add never_return bindgen override for os.Exit and log.Fatal* [`1f78da7`](https://github.com/ivov/lisette/commit/1f78da7bef130afbc264daa7344179e76bc80199)
- fix: off-by-one in struct-literal lookahead skipped empty {} [#132](https://github.com/ivov/lisette/pull/132) [`f419749`](https://github.com/ivov/lisette/commit/f4197492391b1bec1dcafedaf9c32ff96e4470a3)
- fix: support building from source on windows [#130](https://github.com/ivov/lisette/pull/130) [`35c0437`](https://github.com/ivov/lisette/commit/35c04379bf2f4a527ab6d4972ac09af2fe8a2503)
- ci: sort changelog commits by timestamp [`f7fdc64`](https://github.com/ivov/lisette/commit/f7fdc64a18a38ff1b391253ce49313f3f7d7cfbb)

## [0.1.13](https://github.com/ivov/lisette/compare/lisette-v0.1.12...lisette-v0.1.13) - 2026-04-18

- fix: allow closures returning concrete types as Go function aliases [`96b5a31`](https://github.com/ivov/lisette/commit/96b5a3122c85c542176ef7303c63ea70901145cf)
- fix: reverse commit order in changelog template [`9bb906a`](https://github.com/ivov/lisette/commit/9bb906a5dade4e66583177c6011f84fffa374974)
- fix: classify nullable Go function aliases as NullableReturn [`d2fd7f6`](https://github.com/ivov/lisette/commit/d2fd7f61022c66aad3cbf755849162c61d96bd1f)
- ci: open github issue on fuzz crash [`997f63c`](https://github.com/ivov/lisette/commit/997f63c4c42ba25fa9986b835e36c7712db9d4ab)
- fix: desugar pipeline operator inside slice literals [`198bfaa`](https://github.com/ivov/lisette/commit/198bfaacfd2b644ad7785fcecc9488669c673e58)
- fix: preserve Go interface and alias types in match-arm tuple slots [`d73fdbe`](https://github.com/ivov/lisette/commit/d73fdbe6ae6fe076a6a8fe94799443f0d98a5800)
- fix: treat .d.lis types as public in register_module [`95d5704`](https://github.com/ivov/lisette/commit/95d5704fefecc4c68dfff9bb42f835dc20004cf3)
- feat: add byte_at and rune_at to string [#123](https://github.com/ivov/lisette/pull/123) [`c2188a3`](https://github.com/ivov/lisette/commit/c2188a3aa29f7f15595c50d7aedf35a1f152a2e3)
- fix: preserve type alias names in emitter output [#122](https://github.com/ivov/lisette/pull/122) [`49a0817`](https://github.com/ivov/lisette/commit/49a081719deeb679dd247b68a984c218bb92705b)
- fix: don't flag wildcard as redundant in interface match [#121](https://github.com/ivov/lisette/pull/121) [`86dcafd`](https://github.com/ivov/lisette/commit/86dcafd04ab5217137ea5d888a919f46da30743c)
- fix: type os.Exit and log.Fatal* as Never [#120](https://github.com/ivov/lisette/pull/120) [`7dd3e71`](https://github.com/ivov/lisette/commit/7dd3e71960ca51d1477f269d7b17e388f862f9b8)
- fix: widen panic parameter from string to Unknown [#117](https://github.com/ivov/lisette/pull/117) [`489b2db`](https://github.com/ivov/lisette/commit/489b2db09a6ee71bb4a5904e0fb79f299b1fda1e)
- fix: formatter moves comments into impl, try, and recover blocks [#115](https://github.com/ivov/lisette/pull/115) [`b7d1f3a`](https://github.com/ivov/lisette/commit/b7d1f3a9508f1fd75eac2cdeaaf151ab6efc08dd)
- fix: emit value equality for Go sentinel patterns like Err(io.EOF) [#108](https://github.com/ivov/lisette/pull/108) [`bf3d0da`](https://github.com/ivov/lisette/commit/bf3d0dab4bd990c80b305453df2212edc51c4cb6)
- fix: use declared package name over path segment as Go qualifier [#106](https://github.com/ivov/lisette/pull/106) [`09a76f0`](https://github.com/ivov/lisette/commit/09a76f0be5ca203f98b3bbfe94dccdf6c7afc234)
- docs: add Channel<()> signaling example to concurrency reference [#105](https://github.com/ivov/lisette/pull/105) [`1618bb3`](https://github.com/ivov/lisette/commit/1618bb302873008f3ec69c7af98330c107a22799)
- fix: emit break after switch in guarded match [#104](https://github.com/ivov/lisette/pull/104) [`1644ca0`](https://github.com/ivov/lisette/commit/1644ca04959b517743c299a025a0619d5cfa8a4f)
- fix: don't wrap named function type returns in Option [#103](https://github.com/ivov/lisette/pull/103) [`0e31195`](https://github.com/ivov/lisette/commit/0e311953357d3a7df4dac182dee7693e2bc7caa7)
- fix: value enum match arms and interface-slotted tuple returns [#101](https://github.com/ivov/lisette/pull/101) [`2791a57`](https://github.com/ivov/lisette/commit/2791a5720d8b42dc79d736f9e7d90925ce8304c7)

## [0.1.12](https://github.com/ivov/lisette/compare/lisette-v0.1.11...lisette-v0.1.12) - 2026-04-15

- fix: synthesize Go interface adapters for Lisette impls [#92](https://github.com/ivov/lisette/pull/92) [`ccea037`](https://github.com/ivov/lisette/commit/ccea03769210d8995102686e58065710f7318d41)
- fix: emit missing imports for enum variant payload types [#83](https://github.com/ivov/lisette/pull/83) [`c663661`](https://github.com/ivov/lisette/commit/c6636612d7da8f5933f222484655bebc80750251)
- refactor: extract shared go output + finalize helpers [#91](https://github.com/ivov/lisette/pull/91) [`b4ceb49`](https://github.com/ivov/lisette/commit/b4ceb49c7914a926590fd2fd5b505f55e5238c02)
- docs: note that goland extension is available [#89](https://github.com/ivov/lisette/pull/89) [`bad3dee`](https://github.com/ivov/lisette/commit/bad3deea6fd89ddd38ba583f06822f78f7f92ecf)
- fix: regenerate missing Go typedefs before check/build/run [#88](https://github.com/ivov/lisette/pull/88) [`cc6912b`](https://github.com/ivov/lisette/commit/cc6912be7cd3ef069468d1b668c81d72dff58bcb)
- fix: emit named empty Go interfaces as Lisette interfaces [#86](https://github.com/ivov/lisette/pull/86) [`029bb6e`](https://github.com/ivov/lisette/commit/029bb6e55888f0ac59acc9293250e1a88f4ee9b8)

## [0.1.11](https://github.com/ivov/lisette/compare/lisette-v0.1.10...lisette-v0.1.11) - 2026-04-14

- fix: emit unloadable stub when bindgen hits cgo type errors [`82d1022`](https://github.com/ivov/lisette/commit/82d10222af3d2788dd1d5292817e5a507a6b5a9d)
- fix: cap bindgen return-tuple arity at parser limit [`a599e19`](https://github.com/ivov/lisette/commit/a599e193875ea0718da2958750be0a263f5b58fa)
- fix: only translate invalid version errors pinned to user target [`76c0037`](https://github.com/ivov/lisette/commit/76c0037a584f22b1d9835e935d5a61b37397ec4d)
- fix: reject unparseable bindgen output before caching in lis add [`e6fbc1f`](https://github.com/ivov/lisette/commit/e6fbc1f34a7314561980f7007eae8e946d8c5ade)
- fix: emit opaque placeholder for skipped top-level types in bindgen [`e1ef036`](https://github.com/ivov/lisette/commit/e1ef036c88a6610e4ec3560cb8d5eb0fe746dc9b)
- fix: prune stale .go files from target on rebuild [#82](https://github.com/ivov/lisette/pull/82) [`25c1d58`](https://github.com/ivov/lisette/commit/25c1d5807f04c032d24376e400208dcebe3d01dd)
- fix: distinguish package-local Option/Result/Partial from prelude [`3342aa8`](https://github.com/ivov/lisette/commit/3342aa8360830474a035f70a58c3cb071cb6cccb)
- fix: stop prefixing commit hashes with v in lis add [`47196ec`](https://github.com/ivov/lisette/commit/47196ecf0bd4fb4a1bc932f2eae6b9b6db53d2d6)
- fix: preserve snake_case field name on ref receiver access [#80](https://github.com/ivov/lisette/pull/80) [`1fa6205`](https://github.com/ivov/lisette/commit/1fa6205d26c5ea94996091660f90caca6eb39842)
- fix: report missing repo segment in github.com module path [`00f297c`](https://github.com/ivov/lisette/commit/00f297c7f9bdf7b5738375705cb9bdce6c29fdab)
- docs: mention goland in changelog [`56aaff5`](https://github.com/ivov/lisette/commit/56aaff581355f85cc29634d32f250bc48fda6198)

## [0.1.10](https://github.com/ivov/lisette/compare/lisette-v0.1.9...lisette-v0.1.10) - 2026-04-14

- docs: mention goland in homepage [`834c1d3`](https://github.com/ivov/lisette/commit/834c1d31e734012da93f77af18f851376ce12b39)
- feat: goland support [#76](https://github.com/ivov/lisette/pull/76) [`59ef661`](https://github.com/ivov/lisette/commit/59ef6616272b29483f5ef5edfd6edac159c1176d)
- chore: adjust release PR title [`c692dd8`](https://github.com/ivov/lisette/commit/c692dd8360055dd6950f5a0756985c226df6d493)
- chore: simplify release PR body template [#75](https://github.com/ivov/lisette/pull/75) [`bd2fd9b`](https://github.com/ivov/lisette/commit/bd2fd9b1083b4237838de1901d99a03517a6445c)
- fix: accept \a \b \f \v escape sequences in string and rune literals [#73](https://github.com/ivov/lisette/pull/73) [`7b7d7ce`](https://github.com/ivov/lisette/commit/7b7d7ce4d8bd8b8d5ae1c8dc828fcc4a5377dee5)
- fix: default Go import alias to declared package name [#72](https://github.com/ivov/lisette/pull/72) [`af71eca`](https://github.com/ivov/lisette/commit/af71ecacc5fbe85ec02851aa12244be3202f6b59)

## [0.1.9](https://github.com/ivov/lisette/compare/lisette-v0.1.8...lisette-v0.1.9) - 2026-04-13

- fix: reject static method called on an instance [#69](https://github.com/ivov/lisette/pull/69) [`efacd5f`](https://github.com/ivov/lisette/commit/efacd5f42a9f349806c7fd2c8096abe017ebebe7)
- fix: erase self-referential bounds on interface type parameters [#68](https://github.com/ivov/lisette/pull/68) [`a92f8df`](https://github.com/ivov/lisette/commit/a92f8df96afa360f1b5fb3ee3450b30f44d94379)
- fix: allow type alias to fn as type conversion [#65](https://github.com/ivov/lisette/pull/65) [`b806427`](https://github.com/ivov/lisette/commit/b806427096288fc2b39051eae4aaa7e518c06298)
- fix: harden lis add command [#64](https://github.com/ivov/lisette/pull/64) [`f8df4fb`](https://github.com/ivov/lisette/commit/f8df4fb9a35c01d5ec4f00d8345cfa0bde464a50)
- docs: note Zed extension is available [`e97d75c`](https://github.com/ivov/lisette/commit/e97d75c17fcd3c0173ebd9c82fd8d85422227057)
- ci: drop wasm32-wasip2 target from rust-toolchain.toml [`22b4a06`](https://github.com/ivov/lisette/commit/22b4a0648de9ad0acc5dedb0b55e826a51f73a2b)
- ci: add wasm32-wasip2 target for zed extension build [`f80e651`](https://github.com/ivov/lisette/commit/f80e651a1880267d271d6e8a11748ea416696c93)
- chore: symlink license into zed extension folder [`f98a844`](https://github.com/ivov/lisette/commit/f98a844ef3f4b63108d4c97f3a62d8dd4b032c16)
- fix: integer literal edge cases and unicode escape validation [`3b7a2b9`](https://github.com/ivov/lisette/commit/3b7a2b9ca650984bf2547ebc8c24a72f51a7abd5)
- fix: skip bindgen exports referencing internal package types [#59](https://github.com/ivov/lisette/pull/59) [`3956c00`](https://github.com/ivov/lisette/commit/3956c00b34c3aa3c03f9d1995d7ffd0d87128140)
- ci: rename publish job to cover Go modules [`e65bb69`](https://github.com/ivov/lisette/commit/e65bb69b012132eaf1649fd1756d6705a1ba7bc1)
- chore: normalize changelog and fix template format [`e51e64e`](https://github.com/ivov/lisette/commit/e51e64e8aa9abcce40a2a5f52a34eb590acf5aa8)

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

## [0.1.0](https://github.com/ivov/lisette/compare/...lisette-v0.1.0) - 2026-03-21

- feat: initial release v0.1.0 [`a2fbd9d`](https://github.com/ivov/lisette/commit/a2fbd9d956ba38f52a456c5ad51da30e4bacdd1f)

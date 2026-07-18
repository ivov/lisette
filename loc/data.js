window.BENCHMARK_DATA = {
  "lastUpdate": 1784378456969,
  "repoUrl": "https://github.com/ivov/lisette",
  "entries": {
    "production-loc": [
      {
        "commit": {
          "id": "a2fbd9d956ba38f52a456c5ad51da30e4bacdd1f",
          "message": "feat: initial release v0.1.0",
          "timestamp": "2026-03-21T16:59:16+01:00",
          "url": "https://github.com/ivov/lisette/commit/a2fbd9d956ba38f52a456c5ad51da30e4bacdd1f"
        },
        "date": 1774108756000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 55060
          },
          {
            "name": "semantics",
            "unit": "lines",
            "value": 17675
          },
          {
            "name": "emit",
            "unit": "lines",
            "value": 14381
          },
          {
            "name": "syntax",
            "unit": "lines",
            "value": 9779
          },
          {
            "name": "cli",
            "unit": "lines",
            "value": 3816
          },
          {
            "name": "lsp",
            "unit": "lines",
            "value": 3208
          },
          {
            "name": "diagnostics",
            "unit": "lines",
            "value": 3171
          },
          {
            "name": "format",
            "unit": "lines",
            "value": 2051
          },
          {
            "name": "stdlib",
            "unit": "lines",
            "value": 488
          },
          {
            "name": "prelude",
            "unit": "lines",
            "value": 491
          }
        ]
      },
      {
        "commit": {
          "id": "1ab2b6cff453f6484dc504e5e09debcf8048b3f5",
          "message": "fix: lower parser max depth to 64 to prevent stack overflow",
          "timestamp": "2026-03-23T08:24:48+01:00",
          "url": "https://github.com/ivov/lisette/commit/1ab2b6cff453f6484dc504e5e09debcf8048b3f5"
        },
        "date": 1774250688000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 55064
          },
          {
            "name": "semantics",
            "unit": "lines",
            "value": 17675
          },
          {
            "name": "emit",
            "unit": "lines",
            "value": 14381
          },
          {
            "name": "syntax",
            "unit": "lines",
            "value": 9779
          },
          {
            "name": "cli",
            "unit": "lines",
            "value": 3820
          },
          {
            "name": "lsp",
            "unit": "lines",
            "value": 3208
          },
          {
            "name": "diagnostics",
            "unit": "lines",
            "value": 3171
          },
          {
            "name": "format",
            "unit": "lines",
            "value": 2051
          },
          {
            "name": "stdlib",
            "unit": "lines",
            "value": 488
          },
          {
            "name": "prelude",
            "unit": "lines",
            "value": 491
          }
        ]
      },
      {
        "commit": {
          "id": "2d357f179f8f4536b5bc723fad55b438dc2113cf",
          "message": "fix: fold Range sub-expressions in AstFolder",
          "timestamp": "2026-03-31T18:03:37+02:00",
          "url": "https://github.com/ivov/lisette/commit/2d357f179f8f4536b5bc723fad55b438dc2113cf"
        },
        "date": 1774973017000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 55079
          },
          {
            "name": "semantics",
            "unit": "lines",
            "value": 17675
          },
          {
            "name": "emit",
            "unit": "lines",
            "value": 14381
          },
          {
            "name": "syntax",
            "unit": "lines",
            "value": 9796
          },
          {
            "name": "cli",
            "unit": "lines",
            "value": 3818
          },
          {
            "name": "lsp",
            "unit": "lines",
            "value": 3208
          },
          {
            "name": "diagnostics",
            "unit": "lines",
            "value": 3171
          },
          {
            "name": "format",
            "unit": "lines",
            "value": 2051
          },
          {
            "name": "stdlib",
            "unit": "lines",
            "value": 488
          },
          {
            "name": "prelude",
            "unit": "lines",
            "value": 491
          }
        ]
      },
      {
        "commit": {
          "id": "d7e91033ae2be001b029b2f310eb25af6d395243",
          "message": "refactor: replace DiscardedTailFact boolean with enum",
          "timestamp": "2026-04-06T12:04:55+02:00",
          "url": "https://github.com/ivov/lisette/commit/d7e91033ae2be001b029b2f310eb25af6d395243"
        },
        "date": 1775469895000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 55159
          },
          {
            "name": "semantics",
            "unit": "lines",
            "value": 17687
          },
          {
            "name": "emit",
            "unit": "lines",
            "value": 14409
          },
          {
            "name": "syntax",
            "unit": "lines",
            "value": 9802
          },
          {
            "name": "cli",
            "unit": "lines",
            "value": 3828
          },
          {
            "name": "lsp",
            "unit": "lines",
            "value": 3208
          },
          {
            "name": "diagnostics",
            "unit": "lines",
            "value": 3181
          },
          {
            "name": "format",
            "unit": "lines",
            "value": 2051
          },
          {
            "name": "stdlib",
            "unit": "lines",
            "value": 488
          },
          {
            "name": "prelude",
            "unit": "lines",
            "value": 505
          }
        ]
      },
      {
        "commit": {
          "id": "f8df4fb9a35c01d5ec4f00d8345cfa0bde464a50",
          "message": "fix: harden lis add command (#64)",
          "timestamp": "2026-04-13T18:16:46+02:00",
          "url": "https://github.com/ivov/lisette/commit/f8df4fb9a35c01d5ec4f00d8345cfa0bde464a50"
        },
        "date": 1776097006000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 60773
          },
          {
            "name": "semantics",
            "unit": "lines",
            "value": 17886
          },
          {
            "name": "emit",
            "unit": "lines",
            "value": 14579
          },
          {
            "name": "syntax",
            "unit": "lines",
            "value": 9918
          },
          {
            "name": "cli",
            "unit": "lines",
            "value": 5062
          },
          {
            "name": "diagnostics",
            "unit": "lines",
            "value": 3248
          },
          {
            "name": "lsp",
            "unit": "lines",
            "value": 3226
          },
          {
            "name": "format",
            "unit": "lines",
            "value": 2051
          },
          {
            "name": "stdlib",
            "unit": "lines",
            "value": 488
          },
          {
            "name": "deps",
            "unit": "lines",
            "value": 377
          },
          {
            "name": "bindgen",
            "unit": "lines",
            "value": 3354
          },
          {
            "name": "prelude",
            "unit": "lines",
            "value": 584
          }
        ]
      },
      {
        "commit": {
          "id": "9803025475bfd7efb70e91176784887a8387d023",
          "message": "fix: emit Go type switch when matching on an interface type (#138)",
          "timestamp": "2026-04-20T00:39:49+02:00",
          "url": "https://github.com/ivov/lisette/commit/9803025475bfd7efb70e91176784887a8387d023"
        },
        "date": 1776638389000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 62774
          },
          {
            "name": "semantics",
            "unit": "lines",
            "value": 18564
          },
          {
            "name": "emit",
            "unit": "lines",
            "value": 15331
          },
          {
            "name": "syntax",
            "unit": "lines",
            "value": 10032
          },
          {
            "name": "cli",
            "unit": "lines",
            "value": 5277
          },
          {
            "name": "diagnostics",
            "unit": "lines",
            "value": 3309
          },
          {
            "name": "lsp",
            "unit": "lines",
            "value": 3250
          },
          {
            "name": "format",
            "unit": "lines",
            "value": 2098
          },
          {
            "name": "stdlib",
            "unit": "lines",
            "value": 487
          },
          {
            "name": "deps",
            "unit": "lines",
            "value": 377
          },
          {
            "name": "bindgen",
            "unit": "lines",
            "value": 3455
          },
          {
            "name": "prelude",
            "unit": "lines",
            "value": 594
          }
        ]
      },
      {
        "commit": {
          "id": "9681887c997b4c3d6e63e4107d883294edf3e679",
          "message": "feat: zero-fill spread (#210)",
          "timestamp": "2026-04-28T00:57:49+02:00",
          "url": "https://github.com/ivov/lisette/commit/9681887c997b4c3d6e63e4107d883294edf3e679"
        },
        "date": 1777330669000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 70176
          },
          {
            "name": "semantics",
            "unit": "lines",
            "value": 22032
          },
          {
            "name": "emit",
            "unit": "lines",
            "value": 17813
          },
          {
            "name": "syntax",
            "unit": "lines",
            "value": 10703
          },
          {
            "name": "cli",
            "unit": "lines",
            "value": 5450
          },
          {
            "name": "diagnostics",
            "unit": "lines",
            "value": 3594
          },
          {
            "name": "lsp",
            "unit": "lines",
            "value": 3267
          },
          {
            "name": "format",
            "unit": "lines",
            "value": 2117
          },
          {
            "name": "stdlib",
            "unit": "lines",
            "value": 487
          },
          {
            "name": "deps",
            "unit": "lines",
            "value": 449
          },
          {
            "name": "bindgen",
            "unit": "lines",
            "value": 3638
          },
          {
            "name": "prelude",
            "unit": "lines",
            "value": 626
          }
        ]
      },
      {
        "commit": {
          "id": "9b99b08ae61f666d8437e9eedad6b454dc83f118",
          "message": "fix: redirect unwrap/expect diagnostic on Option/Result/Partial (#294)",
          "timestamp": "2026-05-04T07:53:58+02:00",
          "url": "https://github.com/ivov/lisette/commit/9b99b08ae61f666d8437e9eedad6b454dc83f118"
        },
        "date": 1777874038000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 73109
          },
          {
            "name": "semantics",
            "unit": "lines",
            "value": 22199
          },
          {
            "name": "emit",
            "unit": "lines",
            "value": 18028
          },
          {
            "name": "syntax",
            "unit": "lines",
            "value": 10922
          },
          {
            "name": "cli",
            "unit": "lines",
            "value": 5776
          },
          {
            "name": "diagnostics",
            "unit": "lines",
            "value": 3736
          },
          {
            "name": "lsp",
            "unit": "lines",
            "value": 3286
          },
          {
            "name": "format",
            "unit": "lines",
            "value": 2750
          },
          {
            "name": "stdlib",
            "unit": "lines",
            "value": 679
          },
          {
            "name": "deps",
            "unit": "lines",
            "value": 485
          },
          {
            "name": "bindgen",
            "unit": "lines",
            "value": 4616
          },
          {
            "name": "prelude",
            "unit": "lines",
            "value": 632
          }
        ]
      },
      {
        "commit": {
          "id": "69179a98a55b1438a51959d4e0d82c8e1fca1b74",
          "message": "refactor: trim dead temps and discards from emitted Go (#383)",
          "timestamp": "2026-05-11T17:48:13+02:00",
          "url": "https://github.com/ivov/lisette/commit/69179a98a55b1438a51959d4e0d82c8e1fca1b74"
        },
        "date": 1778514493000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 76104
          },
          {
            "name": "semantics",
            "unit": "lines",
            "value": 23125
          },
          {
            "name": "emit",
            "unit": "lines",
            "value": 18672
          },
          {
            "name": "syntax",
            "unit": "lines",
            "value": 11031
          },
          {
            "name": "cli",
            "unit": "lines",
            "value": 6484
          },
          {
            "name": "diagnostics",
            "unit": "lines",
            "value": 3855
          },
          {
            "name": "lsp",
            "unit": "lines",
            "value": 3341
          },
          {
            "name": "format",
            "unit": "lines",
            "value": 2787
          },
          {
            "name": "stdlib",
            "unit": "lines",
            "value": 682
          },
          {
            "name": "deps",
            "unit": "lines",
            "value": 605
          },
          {
            "name": "bindgen",
            "unit": "lines",
            "value": 4826
          },
          {
            "name": "prelude",
            "unit": "lines",
            "value": 696
          }
        ]
      },
      {
        "commit": {
          "id": "4a02f60db605731729688f21644fd8b8c8754033",
          "message": "fix: qualify imported enum in variant-not-found diagnostic (#442)",
          "timestamp": "2026-05-18T22:05:14+02:00",
          "url": "https://github.com/ivov/lisette/commit/4a02f60db605731729688f21644fd8b8c8754033"
        },
        "date": 1779134714000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 80135
          },
          {
            "name": "semantics",
            "unit": "lines",
            "value": 23148
          },
          {
            "name": "emit",
            "unit": "lines",
            "value": 21975
          },
          {
            "name": "syntax",
            "unit": "lines",
            "value": 11163
          },
          {
            "name": "cli",
            "unit": "lines",
            "value": 6659
          },
          {
            "name": "diagnostics",
            "unit": "lines",
            "value": 3903
          },
          {
            "name": "lsp",
            "unit": "lines",
            "value": 3341
          },
          {
            "name": "format",
            "unit": "lines",
            "value": 2787
          },
          {
            "name": "stdlib",
            "unit": "lines",
            "value": 682
          },
          {
            "name": "deps",
            "unit": "lines",
            "value": 605
          },
          {
            "name": "bindgen",
            "unit": "lines",
            "value": 5176
          },
          {
            "name": "prelude",
            "unit": "lines",
            "value": 696
          }
        ]
      },
      {
        "commit": {
          "id": "9f16eef49df5a174c9139656ec5e128611758ec1",
          "message": "fix: disambiguate EcoString as_ref calls with as_str (#482)",
          "timestamp": "2026-05-25T16:55:40+02:00",
          "url": "https://github.com/ivov/lisette/commit/9f16eef49df5a174c9139656ec5e128611758ec1"
        },
        "date": 1779720940000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 82019
          },
          {
            "name": "semantics",
            "unit": "lines",
            "value": 23817
          },
          {
            "name": "emit",
            "unit": "lines",
            "value": 22338
          },
          {
            "name": "syntax",
            "unit": "lines",
            "value": 11293
          },
          {
            "name": "cli",
            "unit": "lines",
            "value": 6935
          },
          {
            "name": "diagnostics",
            "unit": "lines",
            "value": 4087
          },
          {
            "name": "lsp",
            "unit": "lines",
            "value": 3462
          },
          {
            "name": "format",
            "unit": "lines",
            "value": 2794
          },
          {
            "name": "stdlib",
            "unit": "lines",
            "value": 682
          },
          {
            "name": "deps",
            "unit": "lines",
            "value": 605
          },
          {
            "name": "bindgen",
            "unit": "lines",
            "value": 5310
          },
          {
            "name": "prelude",
            "unit": "lines",
            "value": 696
          }
        ]
      },
      {
        "commit": {
          "id": "a5f94c60d530ef9943a2d26c48db921ebdb5c34d",
          "message": "feat: warn on lost url query mutation (#564)",
          "timestamp": "2026-06-01T18:38:40+02:00",
          "url": "https://github.com/ivov/lisette/commit/a5f94c60d530ef9943a2d26c48db921ebdb5c34d"
        },
        "date": 1780331920000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 90167
          },
          {
            "name": "emit",
            "unit": "lines",
            "value": 27470
          },
          {
            "name": "semantics",
            "unit": "lines",
            "value": 26149
          },
          {
            "name": "syntax",
            "unit": "lines",
            "value": 11364
          },
          {
            "name": "cli",
            "unit": "lines",
            "value": 6929
          },
          {
            "name": "diagnostics",
            "unit": "lines",
            "value": 4437
          },
          {
            "name": "lsp",
            "unit": "lines",
            "value": 3507
          },
          {
            "name": "format",
            "unit": "lines",
            "value": 2794
          },
          {
            "name": "stdlib",
            "unit": "lines",
            "value": 682
          },
          {
            "name": "deps",
            "unit": "lines",
            "value": 605
          },
          {
            "name": "benchmark",
            "unit": "lines",
            "value": 212
          },
          {
            "name": "bindgen",
            "unit": "lines",
            "value": 5322
          },
          {
            "name": "prelude",
            "unit": "lines",
            "value": 696
          }
        ]
      },
      {
        "commit": {
          "id": "ee4f66a18635437ec5dc7013332ee675d8d4fa9e",
          "message": "fix: keep f-string interpolations single-line (#657)",
          "timestamp": "2026-06-08T18:19:34+02:00",
          "url": "https://github.com/ivov/lisette/commit/ee4f66a18635437ec5dc7013332ee675d8d4fa9e"
        },
        "date": 1780935574000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 93029
          },
          {
            "name": "semantics",
            "unit": "lines",
            "value": 28789
          },
          {
            "name": "emit",
            "unit": "lines",
            "value": 26663
          },
          {
            "name": "syntax",
            "unit": "lines",
            "value": 11432
          },
          {
            "name": "cli",
            "unit": "lines",
            "value": 6954
          },
          {
            "name": "diagnostics",
            "unit": "lines",
            "value": 4856
          },
          {
            "name": "lsp",
            "unit": "lines",
            "value": 3904
          },
          {
            "name": "format",
            "unit": "lines",
            "value": 2755
          },
          {
            "name": "stdlib",
            "unit": "lines",
            "value": 682
          },
          {
            "name": "deps",
            "unit": "lines",
            "value": 605
          },
          {
            "name": "benchmark",
            "unit": "lines",
            "value": 212
          },
          {
            "name": "bindgen",
            "unit": "lines",
            "value": 5475
          },
          {
            "name": "prelude",
            "unit": "lines",
            "value": 702
          }
        ]
      },
      {
        "commit": {
          "id": "4b54e8ad23be9fe8c0263ce70e1e64258d1d4026",
          "message": "feat: lint for min/max clamp mistakes (#726)",
          "timestamp": "2026-06-15T18:15:24+02:00",
          "url": "https://github.com/ivov/lisette/commit/4b54e8ad23be9fe8c0263ce70e1e64258d1d4026"
        },
        "date": 1781540124000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 96788
          },
          {
            "name": "semantics",
            "unit": "lines",
            "value": 31430
          },
          {
            "name": "emit",
            "unit": "lines",
            "value": 26628
          },
          {
            "name": "syntax",
            "unit": "lines",
            "value": 11663
          },
          {
            "name": "cli",
            "unit": "lines",
            "value": 7217
          },
          {
            "name": "diagnostics",
            "unit": "lines",
            "value": 5070
          },
          {
            "name": "lsp",
            "unit": "lines",
            "value": 4017
          },
          {
            "name": "format",
            "unit": "lines",
            "value": 2757
          },
          {
            "name": "deps",
            "unit": "lines",
            "value": 710
          },
          {
            "name": "stdlib",
            "unit": "lines",
            "value": 682
          },
          {
            "name": "bindgen",
            "unit": "lines",
            "value": 5906
          },
          {
            "name": "prelude",
            "unit": "lines",
            "value": 708
          }
        ]
      },
      {
        "commit": {
          "id": "aa298840f1cd446115521109fd20af36b9be979a",
          "message": "feat: support skipping tests with `t.skip` (#819)",
          "timestamp": "2026-06-22T06:52:37+02:00",
          "url": "https://github.com/ivov/lisette/commit/aa298840f1cd446115521109fd20af36b9be979a"
        },
        "date": 1782103957000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 104677
          },
          {
            "name": "emit",
            "unit": "lines",
            "value": 27313
          },
          {
            "name": "semantics",
            "unit": "lines",
            "value": 21122
          },
          {
            "name": "passes",
            "unit": "lines",
            "value": 14659
          },
          {
            "name": "syntax",
            "unit": "lines",
            "value": 12183
          },
          {
            "name": "cli",
            "unit": "lines",
            "value": 8485
          },
          {
            "name": "diagnostics",
            "unit": "lines",
            "value": 5644
          },
          {
            "name": "lsp",
            "unit": "lines",
            "value": 4275
          },
          {
            "name": "format",
            "unit": "lines",
            "value": 2769
          },
          {
            "name": "deps",
            "unit": "lines",
            "value": 781
          },
          {
            "name": "stdlib",
            "unit": "lines",
            "value": 695
          },
          {
            "name": "bindgen",
            "unit": "lines",
            "value": 5934
          },
          {
            "name": "prelude",
            "unit": "lines",
            "value": 817
          }
        ]
      },
      {
        "commit": {
          "id": "b1206f2741f40ea5c82b20c039aa184bf124285c",
          "message": "refactor: reduce cyclomatic complexity in emit crate (#914)",
          "timestamp": "2026-06-29T18:54:58+02:00",
          "url": "https://github.com/ivov/lisette/commit/b1206f2741f40ea5c82b20c039aa184bf124285c"
        },
        "date": 1782752098000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 106747
          },
          {
            "name": "emit",
            "unit": "lines",
            "value": 27355
          },
          {
            "name": "semantics",
            "unit": "lines",
            "value": 21594
          },
          {
            "name": "passes",
            "unit": "lines",
            "value": 14785
          },
          {
            "name": "syntax",
            "unit": "lines",
            "value": 12461
          },
          {
            "name": "cli",
            "unit": "lines",
            "value": 9358
          },
          {
            "name": "diagnostics",
            "unit": "lines",
            "value": 5742
          },
          {
            "name": "lsp",
            "unit": "lines",
            "value": 4243
          },
          {
            "name": "format",
            "unit": "lines",
            "value": 2768
          },
          {
            "name": "deps",
            "unit": "lines",
            "value": 785
          },
          {
            "name": "stdlib",
            "unit": "lines",
            "value": 695
          },
          {
            "name": "bindgen",
            "unit": "lines",
            "value": 6095
          },
          {
            "name": "prelude",
            "unit": "lines",
            "value": 866
          }
        ]
      },
      {
        "commit": {
          "id": "039eb73d60dd20267479aa134a14f15ba729849d",
          "message": "fix: emit pointer indirection for indirect enum recursion (#966)",
          "timestamp": "2026-07-06T19:53:41+02:00",
          "url": "https://github.com/ivov/lisette/commit/039eb73d60dd20267479aa134a14f15ba729849d"
        },
        "date": 1783360421000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 110418
          },
          {
            "name": "emit",
            "unit": "lines",
            "value": 27513
          },
          {
            "name": "semantics",
            "unit": "lines",
            "value": 22087
          },
          {
            "name": "passes",
            "unit": "lines",
            "value": 16273
          },
          {
            "name": "syntax",
            "unit": "lines",
            "value": 12641
          },
          {
            "name": "cli",
            "unit": "lines",
            "value": 10059
          },
          {
            "name": "diagnostics",
            "unit": "lines",
            "value": 6038
          },
          {
            "name": "lsp",
            "unit": "lines",
            "value": 4313
          },
          {
            "name": "format",
            "unit": "lines",
            "value": 2783
          },
          {
            "name": "deps",
            "unit": "lines",
            "value": 1031
          },
          {
            "name": "stdlib",
            "unit": "lines",
            "value": 695
          },
          {
            "name": "bindgen",
            "unit": "lines",
            "value": 6095
          },
          {
            "name": "prelude",
            "unit": "lines",
            "value": 890
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "7c9da0ea24d3117af8fc5bef095efd882126b015",
          "message": "ci: track production lines of code over time (#981)",
          "timestamp": "2026-07-09T23:12:55+02:00",
          "tree_id": "c06a071f0ad59ef87633a677aa46a7279ef68709",
          "url": "https://github.com/ivov/lisette/commit/7c9da0ea24d3117af8fc5bef095efd882126b015"
        },
        "date": 1783631598273,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 112118,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 27975,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22813,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16443,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 12807,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10078,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6160,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4348,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2783,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6095,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "7123691f88839f427965bbf75d454413d47588e5",
          "message": "ci: rename LoC action (#982)",
          "timestamp": "2026-07-09T23:16:38+02:00",
          "tree_id": "603beb83cfa899fa0c3df2a48cedd218b6d46cbd",
          "url": "https://github.com/ivov/lisette/commit/7123691f88839f427965bbf75d454413d47588e5"
        },
        "date": 1783631837902,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 112118,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 27975,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22813,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16443,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 12807,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10078,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6160,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4348,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2783,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6095,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "bf785370741fc05e376e2479bda2638a9f9c97c7",
          "message": "chore: bump stdlib typedefs to v0.8.0 (#983)",
          "timestamp": "2026-07-09T23:30:09+02:00",
          "tree_id": "6121e0f5e766b8796d6ea7945e22a1a1831233eb",
          "url": "https://github.com/ivov/lisette/commit/bf785370741fc05e376e2479bda2638a9f9c97c7"
        },
        "date": 1783632634024,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 112118,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 27975,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22813,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16443,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 12807,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10078,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6160,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4348,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2783,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6095,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "72f90b5ff4332ea97a3df682bc6fd4aceb2aabbf",
          "message": "fix: reject built-in types as interface values (#984)",
          "timestamp": "2026-07-09T23:42:41+02:00",
          "tree_id": "0a3810ae5197981cd122901345fcbd4d02d0ec3b",
          "url": "https://github.com/ivov/lisette/commit/72f90b5ff4332ea97a3df682bc6fd4aceb2aabbf"
        },
        "date": 1783633383043,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 112169,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 27975,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22848,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16443,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 12807,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10078,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6176,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4348,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2783,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6095,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "storm.patrik@gmail.com",
            "name": "Patrik Storm",
            "username": "stormpat"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "31fdd727c2f3a77d02f2e9eae6d2f94eb5d5fe2f",
          "message": "fix: follow embed promotion in unused struct field lint (#985)",
          "timestamp": "2026-07-10T18:24:58+02:00",
          "tree_id": "7a9be3032a873f577954b5041b2248ffafaaa02a",
          "url": "https://github.com/ivov/lisette/commit/31fdd727c2f3a77d02f2e9eae6d2f94eb5d5fe2f"
        },
        "date": 1783700827467,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 112150,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 27975,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22848,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16424,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 12807,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10078,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6176,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4348,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2783,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6095,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "f5dde91a6b61c96b1be84f6e2f3b0c338f471cfd",
          "message": "perf: parallelize source hashing and test-module discovery (#986)",
          "timestamp": "2026-07-10T18:46:56+02:00",
          "tree_id": "33732d641920fd217eb0af6ba09a1000b60c5cd5",
          "url": "https://github.com/ivov/lisette/commit/f5dde91a6b61c96b1be84f6e2f3b0c338f471cfd"
        },
        "date": 1783702040358,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 112181,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 27975,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22879,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16424,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 12807,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10078,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6176,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4348,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2783,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6095,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "b850c8bbdf395ea9512400fd3e491dd619b5118e",
          "message": "fix: distinguish between same-named structs in unused field lint (#987)",
          "timestamp": "2026-07-10T18:56:56+02:00",
          "tree_id": "11ba4d1947530854e0190b5b183f2919a5e93ea7",
          "url": "https://github.com/ivov/lisette/commit/b850c8bbdf395ea9512400fd3e491dd619b5118e"
        },
        "date": 1783702637734,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 112195,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 27975,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22879,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16438,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 12807,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10078,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6176,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4348,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2783,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6095,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "1e6516bfccb12a5334e2d4d66a28519fdfe6b3a6",
          "message": "refactor: unify how calls resolve their Go signature (#988)",
          "timestamp": "2026-07-10T20:28:09+02:00",
          "tree_id": "8019f0692ca969ef9912a40d1da4f3dacb7f8b8c",
          "url": "https://github.com/ivov/lisette/commit/1e6516bfccb12a5334e2d4d66a28519fdfe6b3a6"
        },
        "date": 1783708110593,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 112320,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 28100,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22879,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16438,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 12807,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10078,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6176,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4348,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2783,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6095,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "6c99aa5dbfff87812a4914e2bc9e771a3e34def7",
          "message": "refactor: record side effects when lowering expressions (#989)",
          "timestamp": "2026-07-10T22:57:28+02:00",
          "tree_id": "148bc48a8d17f3a9e043f3dbd7e0d81f768cf606",
          "url": "https://github.com/ivov/lisette/commit/6c99aa5dbfff87812a4914e2bc9e771a3e34def7"
        },
        "date": 1783717070564,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 113315,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29095,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22879,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16438,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 12807,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10078,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6176,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4348,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2783,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6095,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "ba91e4c69acb145dd950ffbdf98cf4ac7c99880e",
          "message": "feat!: interop for fixed-size arrays (#977)\n\nCo-authored-by: Patrik Storm <storm.patrik@gmail.com>",
          "timestamp": "2026-07-10T23:30:53+02:00",
          "tree_id": "d16aacfd1545cbcec0c6d4448458eefe0aa26be9",
          "url": "https://github.com/ivov/lisette/commit/ba91e4c69acb145dd950ffbdf98cf4ac7c99880e"
        },
        "date": 1783719076764,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 113269,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29040,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22929,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16438,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 12807,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10078,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6184,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4348,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2783,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "01cb446f9271fa366b6c94dba271508e891f0b27",
          "message": "refactor: model recursive Go slot layouts with provenance (#990)",
          "timestamp": "2026-07-11T00:07:38+02:00",
          "tree_id": "bcd540051dfe0fa3255dcbd66228b837a14ac13b",
          "url": "https://github.com/ivov/lisette/commit/01cb446f9271fa366b6c94dba271508e891f0b27"
        },
        "date": 1783721278011,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 114203,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29974,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22929,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16438,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 12807,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10078,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6184,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4348,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2783,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "8fbbd1bc2989f5f1b47215a86c0f3c912f7ef252",
          "message": "fix: convert `Option` values across Go callable boundaries (#991)",
          "timestamp": "2026-07-11T01:09:39+02:00",
          "tree_id": "a25204e1f88dd5f43485e716f7763264170efb79",
          "url": "https://github.com/ivov/lisette/commit/8fbbd1bc2989f5f1b47215a86c0f3c912f7ef252"
        },
        "date": 1783725001368,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 114812,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30583,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22929,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16438,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 12807,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10078,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6184,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4348,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2783,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "5bae4a43c827e88907a9e7e7cb335286210385cf",
          "message": "fix: let Go infer method call type arguments (#993)",
          "timestamp": "2026-07-11T12:29:57+02:00",
          "tree_id": "8042cb08feb655534349871d70b0f9a5938bc5cf",
          "url": "https://github.com/ivov/lisette/commit/5bae4a43c827e88907a9e7e7cb335286210385cf"
        },
        "date": 1783765821030,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 114818,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30589,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22929,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16438,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 12807,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10078,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6184,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4348,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2783,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "32723ee250ad0ca0662b505dac0e6e68b8b769fd",
          "message": "refactor: give each emitted file its own package namespace (#994)",
          "timestamp": "2026-07-11T12:56:26+02:00",
          "tree_id": "949d272f7e1209c15c15071447405f03579ba573",
          "url": "https://github.com/ivov/lisette/commit/32723ee250ad0ca0662b505dac0e6e68b8b769fd"
        },
        "date": 1783767406405,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 114783,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30554,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22929,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16438,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 12807,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10078,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6184,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4348,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2783,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "7325e759329bb2365bdecf95141b129b23dfd632",
          "message": "refactor: preserve lexical control targets when lowering (#995)",
          "timestamp": "2026-07-11T13:58:39+02:00",
          "tree_id": "a4b748c3a9750f1fbf2b1359bb4c8530a8da05f6",
          "url": "https://github.com/ivov/lisette/commit/7325e759329bb2365bdecf95141b129b23dfd632"
        },
        "date": 1783771142159,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 114870,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30759,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22831,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16438,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 12787,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10078,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6184,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4348,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2783,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "0efc0bec291c56990e30b02a9d6d16a5c3b2af1d",
          "message": "feat: polish array API, diagnostics, tooling (#996)",
          "timestamp": "2026-07-11T14:51:47+02:00",
          "tree_id": "7b6aa622ce6e0a6670c9f05cc8097781e029ff1e",
          "url": "https://github.com/ivov/lisette/commit/0efc0bec291c56990e30b02a9d6d16a5c3b2af1d"
        },
        "date": 1783774330328,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 114900,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30759,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22848,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16438,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 12787,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10082,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6190,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4351,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2783,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "c6f515b7cfe4f24b18677161574a880ce2ebf85a",
          "message": "refactor: derive emit facts upstream (#997)",
          "timestamp": "2026-07-11T15:14:37+02:00",
          "tree_id": "8dfc972f3ae3762bc69773df598a723a98d4b632",
          "url": "https://github.com/ivov/lisette/commit/c6f515b7cfe4f24b18677161574a880ce2ebf85a"
        },
        "date": 1783775698892,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 114754,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29815,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22902,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17118,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 12844,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10082,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6197,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4351,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2783,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "75673b48490ae8ee6e45bc5110f885eed77d1b5e",
          "message": "docs: surface fixed-size arrays (#998)",
          "timestamp": "2026-07-11T15:21:22+02:00",
          "tree_id": "c40b0cbddc8f7285c18fd2fd33e23cdeeddf5b35",
          "url": "https://github.com/ivov/lisette/commit/75673b48490ae8ee6e45bc5110f885eed77d1b5e"
        },
        "date": 1783776103667,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 114754,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29815,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22902,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17118,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 12844,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10082,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6197,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4351,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2783,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "a4141d0c3872aeb42612a6d641189ce6c7a7042f",
          "message": "fix!: reject generic bounds that were never declared (#999)",
          "timestamp": "2026-07-11T17:39:41+02:00",
          "tree_id": "4152cbfbb835ebc0a10fdb7fae762c5cb09d5106",
          "url": "https://github.com/ivov/lisette/commit/a4141d0c3872aeb42612a6d641189ce6c7a7042f"
        },
        "date": 1783784401853,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 113966,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29862,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22859,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16401,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 12783,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10082,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6183,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4351,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2783,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "f34a066083fdbceac357b2db17ad4a305f6af155",
          "message": "chore: rebuild playground (#1000)",
          "timestamp": "2026-07-11T19:23:11+02:00",
          "tree_id": "2c82bce2c5ab3785ef936def66c8b831f8f88c07",
          "url": "https://github.com/ivov/lisette/commit/f34a066083fdbceac357b2db17ad4a305f6af155"
        },
        "date": 1783790610356,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 113966,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29862,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22859,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16401,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 12783,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10082,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6183,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4351,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2783,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "a9ea2f856505113708150ab17ee853a43f3a94f3",
          "message": "fix: shut down language server gracefully (#1001)",
          "timestamp": "2026-07-11T20:30:08+02:00",
          "tree_id": "1a1d5eb59f90d80c21c56127e050aa61240239ad",
          "url": "https://github.com/ivov/lisette/commit/a9ea2f856505113708150ab17ee853a43f3a94f3"
        },
        "date": 1783794628867,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 114031,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29862,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22859,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16401,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 12783,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10085,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6183,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4413,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2783,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "2ec24b148397803174d0dac5c9bce56a958be53e",
          "message": "fix: close tree-sitter grammar gaps (#1002)",
          "timestamp": "2026-07-11T22:29:45+02:00",
          "tree_id": "f1352f7fcdb315eeeb6805ed7e81aa468db1ccbe",
          "url": "https://github.com/ivov/lisette/commit/2ec24b148397803174d0dac5c9bce56a958be53e"
        },
        "date": 1783801816410,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 114031,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29862,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22859,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16401,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 12783,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10085,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6183,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4413,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2783,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "dec168e774211405b74ac5ced152557584167bb4",
          "message": "chore: release v0.8.0 (#964)",
          "timestamp": "2026-07-11T22:38:10+02:00",
          "tree_id": "6767c4734860ce3f7ddda7844042e2b89045b553",
          "url": "https://github.com/ivov/lisette/commit/dec168e774211405b74ac5ced152557584167bb4"
        },
        "date": 1783802316339,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 114031,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29862,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22859,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16401,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 12783,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10085,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6183,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4413,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2783,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "fdbffb83f4de2a621e9ec95db0f50f5b7aa245e7",
          "message": "docs: expand array coverage (#1005)",
          "timestamp": "2026-07-12T19:17:34+02:00",
          "tree_id": "73097aaaff35422c74187a6ad8a59b4cb4d20f3a",
          "url": "https://github.com/ivov/lisette/commit/fdbffb83f4de2a621e9ec95db0f50f5b7aa245e7"
        },
        "date": 1783876678032,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 114031,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29862,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22859,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16401,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 12783,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10085,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6183,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4413,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2783,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "735f6e176801cd6086bb6cceae7dbcda0e40359a",
          "message": "fix: stop rejecting valid self-referential generic types (#1010)",
          "timestamp": "2026-07-15T21:09:29+02:00",
          "tree_id": "0a7e7df4c83a43dd17302be26a00f64217136d2f",
          "url": "https://github.com/ivov/lisette/commit/735f6e176801cd6086bb6cceae7dbcda0e40359a"
        },
        "date": 1784142591440,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 114144,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29798,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22808,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16401,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13011,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10085,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6183,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4413,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2783,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "8ad90fa4828caed917317c399f5a43fffe65bf0e",
          "message": "fix: make empty slice literals consistent with constructors (#1011)",
          "timestamp": "2026-07-15T21:17:29+02:00",
          "tree_id": "4642b3e09da0d65921f1a96f86f28fdbb255f907",
          "url": "https://github.com/ivov/lisette/commit/8ad90fa4828caed917317c399f5a43fffe65bf0e"
        },
        "date": 1784143208345,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 114218,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29766,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22841,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16428,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13049,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10085,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6191,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4413,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2783,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "85a72423d892ad450f1746f1a2e3f7603d5e0ec2",
          "message": "feat: file comments (#1013)",
          "timestamp": "2026-07-16T20:56:42+02:00",
          "tree_id": "ab76b01bea4aa86692806f2fc04eccd996ace518",
          "url": "https://github.com/ivov/lisette/commit/85a72423d892ad450f1746f1a2e3f7603d5e0ec2"
        },
        "date": 1784228225383,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 114389,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29770,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22852,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16428,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13174,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10086,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6191,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4414,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2812,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "7debe4e0de82ef96f87e8ebdb115fbf91277b50e",
          "message": "fix: require `mut` for `copy_from` (#1014)",
          "timestamp": "2026-07-16T22:27:40+02:00",
          "tree_id": "3bfc758cb78b0ac1c8a32f374ccc7d868755445b",
          "url": "https://github.com/ivov/lisette/commit/7debe4e0de82ef96f87e8ebdb115fbf91277b50e"
        },
        "date": 1784233681782,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 114393,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29770,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22856,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16428,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13174,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10086,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6191,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4414,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2812,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "14e7636aa255b08a7d7d83f89ebf801f9a7e0578",
          "message": "docs: expand prelude docs (#1015)",
          "timestamp": "2026-07-16T22:36:03+02:00",
          "tree_id": "8ef3dddf39b70cceee5742fc97f0bd17ed50c6a4",
          "url": "https://github.com/ivov/lisette/commit/14e7636aa255b08a7d7d83f89ebf801f9a7e0578"
        },
        "date": 1784234190838,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 114401,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29770,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22856,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16428,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13174,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10094,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6191,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4414,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2812,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "f2290e6f9cdb003e6f1fc0093c3597c65e8f3274",
          "message": "docs: improve `lis doc` formatting (#1016)",
          "timestamp": "2026-07-16T22:56:27+02:00",
          "tree_id": "f189896cc6b3cfa234f44719aa5d3777fa158092",
          "url": "https://github.com/ivov/lisette/commit/f2290e6f9cdb003e6f1fc0093c3597c65e8f3274"
        },
        "date": 1784235411863,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 114439,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29770,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22856,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16428,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13174,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10132,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6191,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4414,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2812,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "4ff43be9787045886ec9aea68c385fb853d95366",
          "message": "fix: prevent unintended mutation from `append` aliasing (#1017)",
          "timestamp": "2026-07-16T23:18:31+02:00",
          "tree_id": "40a58d30502ad72ef771918625bc6fe4d851a56d",
          "url": "https://github.com/ivov/lisette/commit/4ff43be9787045886ec9aea68c385fb853d95366"
        },
        "date": 1784236733180,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 114571,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29902,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22856,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16428,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13174,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10132,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6191,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4414,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2812,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "3c1837a6deaa204ce70ca8c699e1ee6083c4bf77",
          "message": "perf: resolve display paths without per-file syscalls (#1018)",
          "timestamp": "2026-07-17T00:53:45+02:00",
          "tree_id": "275cb98d9a1682dedbab4ea59cde576e9e910d19",
          "url": "https://github.com/ivov/lisette/commit/3c1837a6deaa204ce70ca8c699e1ee6083c4bf77"
        },
        "date": 1784242444825,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 114626,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29902,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22906,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16428,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13174,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10137,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6191,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4414,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2812,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "ba506a32a018995b70964382adcfdec2262db1bb",
          "message": "fix: reject references to interface values (#1023)",
          "timestamp": "2026-07-17T21:17:16+02:00",
          "tree_id": "fa5715832f926c527f11701db61333072626efda",
          "url": "https://github.com/ivov/lisette/commit/ba506a32a018995b70964382adcfdec2262db1bb"
        },
        "date": 1784315857124,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 114691,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29902,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22945,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16428,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13174,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10137,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6217,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4414,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2812,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "f8973d39f46736dc94819a247e5e823799e144d1",
          "message": "fix: stop rejecting generic returns constrained by builtins (#1028)",
          "timestamp": "2026-07-17T21:55:02+02:00",
          "tree_id": "af10d8b5c0ef72d750fbcaa4dfd3da8b5510609f",
          "url": "https://github.com/ivov/lisette/commit/f8973d39f46736dc94819a247e5e823799e144d1"
        },
        "date": 1784318126263,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 114694,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29902,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 22945,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16431,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13174,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10137,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6217,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4414,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2812,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "7519a7008c5fdb1f46fb6a921a07ca89e9ff9763",
          "message": "fix: reject non-constructor names in match patterns (#1029)",
          "timestamp": "2026-07-17T21:56:25+02:00",
          "tree_id": "c6d7a43cea45563bd3939f9cd372ee14d955d401",
          "url": "https://github.com/ivov/lisette/commit/7519a7008c5fdb1f46fb6a921a07ca89e9ff9763"
        },
        "date": 1784318206436,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 114768,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29902,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 23019,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16431,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13174,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10137,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6217,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4414,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2812,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "cf3502d373b7916c22c136c60e9ed1f334637dd8",
          "message": "refactor: reword mutable receiver interface diagnostic (#1031)",
          "timestamp": "2026-07-17T22:44:06+02:00",
          "tree_id": "3ca823ecb12941bf5302efa0d7cbcc5cf77b823c",
          "url": "https://github.com/ivov/lisette/commit/cf3502d373b7916c22c136c60e9ed1f334637dd8"
        },
        "date": 1784321069211,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 114771,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29902,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 23019,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16431,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13174,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10137,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6220,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4414,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2812,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "b99edd6deb91b4fd603d0a29d785583ecba1463a",
          "message": "fix: preserve mutations through `Ref` method receivers (#1032)",
          "timestamp": "2026-07-17T23:29:29+02:00",
          "tree_id": "a268896d642b697ea3055f56fde9f564725b5696",
          "url": "https://github.com/ivov/lisette/commit/b99edd6deb91b4fd603d0a29d785583ecba1463a"
        },
        "date": 1784323789867,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 114794,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29925,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 23019,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16431,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13174,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10137,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6220,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4414,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2812,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "101fd72ebf24c266fd2869eb5042f7a95e49a138",
          "message": "fix!: reject `self` on interface methods (#1033)",
          "timestamp": "2026-07-18T00:27:55+02:00",
          "tree_id": "eaaf8242cd37f929d4023393ce13954466f9fd0d",
          "url": "https://github.com/ivov/lisette/commit/101fd72ebf24c266fd2869eb5042f7a95e49a138"
        },
        "date": 1784327299283,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 114801,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29917,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 23027,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16431,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13175,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10137,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6226,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4414,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2812,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "616025fb2db664b01e43264e6f7b9880ad79b807",
          "message": "feat: `Slice.make` and `Slice.reserve` (#1026)",
          "timestamp": "2026-07-18T13:35:59+02:00",
          "tree_id": "593cc166a7cb0534cc24048f0440c37e40071b44",
          "url": "https://github.com/ivov/lisette/commit/616025fb2db664b01e43264e6f7b9880ad79b807"
        },
        "date": 1784374580760,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 115207,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29963,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 23128,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16621,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13184,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10137,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6286,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4414,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2812,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "06e939ae3e77d7f3bb0a82af59b368293b41312c",
          "message": "fix: preserve interface type in optional field (#1037)",
          "timestamp": "2026-07-18T14:36:56+02:00",
          "tree_id": "f3af38e3265ac9770767970b7dbed387a4741bfa",
          "url": "https://github.com/ivov/lisette/commit/06e939ae3e77d7f3bb0a82af59b368293b41312c"
        },
        "date": 1784378237975,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 115209,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29963,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 23130,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16621,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13184,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10137,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6286,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4414,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2812,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ivov.src@gmail.com",
            "name": "Iván Ovejero",
            "username": "ivov"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "3783b52a0474ad681f2b26824bbf41ab0dd49781",
          "message": "fix: aliased import whose name conflicts with a Go import (#1038)",
          "timestamp": "2026-07-18T14:40:35+02:00",
          "tree_id": "b0f2371b4ef533371d10d41dbc953bd567fbca68",
          "url": "https://github.com/ivov/lisette/commit/3783b52a0474ad681f2b26824bbf41ab0dd49781"
        },
        "date": 1784378455963,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 115210,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29962,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 23132,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16621,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13184,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10137,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6286,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4414,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2812,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1031,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6046,
            "unit": "lines"
          },
          {
            "name": "prelude",
            "value": 890,
            "unit": "lines"
          }
        ]
      }
    ]
  }
}
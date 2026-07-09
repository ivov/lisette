window.BENCHMARK_DATA = {
  "lastUpdate": 0,
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
      }
    ]
  }
}

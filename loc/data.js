window.BENCHMARK_DATA = {
  "lastUpdate": 1785340352654,
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
          "id": "32d8819407e7ce7f0bdf622258fcdb89d7509bb1",
          "message": "ci: enable changelog for main crate with cross-crate commits",
          "timestamp": "2026-03-22T19:13:01+01:00",
          "url": "https://github.com/ivov/lisette/commit/32d8819407e7ce7f0bdf622258fcdb89d7509bb1"
        },
        "date": 1774203181000,
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
          "id": "c123f33fc5c674d96dff66f60622e9bb802b4059",
          "message": "fix: prevent OOM by lowering max parser errors to 50",
          "timestamp": "2026-03-25T20:20:30+01:00",
          "url": "https://github.com/ivov/lisette/commit/c123f33fc5c674d96dff66f60622e9bb802b4059"
        },
        "date": 1774466430000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 55063
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
            "value": 9780
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
          "id": "d3d2fb6e5c674b07bdae1e00d1e772b90bb6d799",
          "message": "chore: release v0.1.2 (#1)",
          "timestamp": "2026-03-31T18:34:12+02:00",
          "url": "https://github.com/ivov/lisette/commit/d3d2fb6e5c674b07bdae1e00d1e772b90bb6d799"
        },
        "date": 1774974852000,
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
          "id": "a1836f40d5ada27db809193801ce5dddfdba92e7",
          "message": "chore: remove .cargo from gitignore",
          "timestamp": "2026-04-04T14:29:22+02:00",
          "url": "https://github.com/ivov/lisette/commit/a1836f40d5ada27db809193801ce5dddfdba92e7"
        },
        "date": 1775305762000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 55147
          },
          {
            "name": "semantics",
            "unit": "lines",
            "value": 17675
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
          "id": "e97d75c17fcd3c0173ebd9c82fd8d85422227057",
          "message": "docs: note Zed extension is available",
          "timestamp": "2026-04-12T23:20:23+02:00",
          "url": "https://github.com/ivov/lisette/commit/e97d75c17fcd3c0173ebd9c82fd8d85422227057"
        },
        "date": 1776028823000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 60190
          },
          {
            "name": "semantics",
            "unit": "lines",
            "value": 17886
          },
          {
            "name": "emit",
            "unit": "lines",
            "value": 14571
          },
          {
            "name": "syntax",
            "unit": "lines",
            "value": 9918
          },
          {
            "name": "cli",
            "unit": "lines",
            "value": 4536
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
            "value": 328
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
          "id": "0fd0365c7b3e569851863d7062c06b3f180eba31",
          "message": "chore: release v0.1.15 (#139)",
          "timestamp": "2026-04-20T00:42:39+02:00",
          "url": "https://github.com/ivov/lisette/commit/0fd0365c7b3e569851863d7062c06b3f180eba31"
        },
        "date": 1776638559000,
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
          "id": "6091db8e03b00bb0d765f52586a483c9da29a8de",
          "message": "feat: support sql.Scanner and driver.Valuer on option (#206)",
          "timestamp": "2026-04-26T23:50:09+02:00",
          "url": "https://github.com/ivov/lisette/commit/6091db8e03b00bb0d765f52586a483c9da29a8de"
        },
        "date": 1777240209000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 69341
          },
          {
            "name": "semantics",
            "unit": "lines",
            "value": 21495
          },
          {
            "name": "emit",
            "unit": "lines",
            "value": 17627
          },
          {
            "name": "syntax",
            "unit": "lines",
            "value": 10668
          },
          {
            "name": "cli",
            "unit": "lines",
            "value": 5450
          },
          {
            "name": "diagnostics",
            "unit": "lines",
            "value": 3524
          },
          {
            "name": "lsp",
            "unit": "lines",
            "value": 3267
          },
          {
            "name": "format",
            "unit": "lines",
            "value": 2110
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
          "id": "1a47728bc2989b05aeb9128efae13d1b8e8a6c15",
          "message": "chore: release v0.1.23 (#238)",
          "timestamp": "2026-04-30T23:29:25+02:00",
          "url": "https://github.com/ivov/lisette/commit/1a47728bc2989b05aeb9128efae13d1b8e8a6c15"
        },
        "date": 1777584565000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 71311
          },
          {
            "name": "semantics",
            "unit": "lines",
            "value": 22101
          },
          {
            "name": "emit",
            "unit": "lines",
            "value": 17828
          },
          {
            "name": "syntax",
            "unit": "lines",
            "value": 10822
          },
          {
            "name": "cli",
            "unit": "lines",
            "value": 5576
          },
          {
            "name": "diagnostics",
            "unit": "lines",
            "value": 3636
          },
          {
            "name": "lsp",
            "unit": "lines",
            "value": 3274
          },
          {
            "name": "format",
            "unit": "lines",
            "value": 2281
          },
          {
            "name": "stdlib",
            "unit": "lines",
            "value": 487
          },
          {
            "name": "deps",
            "unit": "lines",
            "value": 452
          },
          {
            "name": "bindgen",
            "unit": "lines",
            "value": 4228
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
          "id": "a11466ebe80507608852c5cd225e79a97dc7655f",
          "message": "ci: skip regen step when release-plz returns no PR (#246)",
          "timestamp": "2026-05-01T00:06:15+02:00",
          "url": "https://github.com/ivov/lisette/commit/a11466ebe80507608852c5cd225e79a97dc7655f"
        },
        "date": 1777586775000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 71311
          },
          {
            "name": "semantics",
            "unit": "lines",
            "value": 22101
          },
          {
            "name": "emit",
            "unit": "lines",
            "value": 17828
          },
          {
            "name": "syntax",
            "unit": "lines",
            "value": 10822
          },
          {
            "name": "cli",
            "unit": "lines",
            "value": 5576
          },
          {
            "name": "diagnostics",
            "unit": "lines",
            "value": 3636
          },
          {
            "name": "lsp",
            "unit": "lines",
            "value": 3274
          },
          {
            "name": "format",
            "unit": "lines",
            "value": 2281
          },
          {
            "name": "stdlib",
            "unit": "lines",
            "value": 487
          },
          {
            "name": "deps",
            "unit": "lines",
            "value": 452
          },
          {
            "name": "bindgen",
            "unit": "lines",
            "value": 4228
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
          "id": "8f635cceae09fb0bf47100b46c0adfa9b3276b90",
          "message": "fix: ship per-target stdlib typedefs (#291)",
          "timestamp": "2026-05-03T20:48:52+02:00",
          "url": "https://github.com/ivov/lisette/commit/8f635cceae09fb0bf47100b46c0adfa9b3276b90"
        },
        "date": 1777834132000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 73082
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
            "value": 3709
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
          "id": "3f7a395a77c2dab18cda281997a873ec43eeb65f",
          "message": "perf: skip per-file gofmt during build (#381)",
          "timestamp": "2026-05-10T23:36:54+02:00",
          "url": "https://github.com/ivov/lisette/commit/3f7a395a77c2dab18cda281997a873ec43eeb65f"
        },
        "date": 1778449014000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 76023
          },
          {
            "name": "semantics",
            "unit": "lines",
            "value": 23125
          },
          {
            "name": "emit",
            "unit": "lines",
            "value": 18591
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
          "id": "b84010dbb7cf163921bf09f0fb4bdf2fbd28ac95",
          "message": "chore: release v0.2.7 (#421)",
          "timestamp": "2026-05-17T23:53:50+02:00",
          "url": "https://github.com/ivov/lisette/commit/b84010dbb7cf163921bf09f0fb4bdf2fbd28ac95"
        },
        "date": 1779054830000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 80120
          },
          {
            "name": "semantics",
            "unit": "lines",
            "value": 23133
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
          "id": "8bd5bb1ac506423b6143eeac23821a01b8eb4cbb",
          "message": "chore: release v0.2.11 (#478)",
          "timestamp": "2026-05-23T17:32:52+02:00",
          "url": "https://github.com/ivov/lisette/commit/8bd5bb1ac506423b6143eeac23821a01b8eb4cbb"
        },
        "date": 1779550372000,
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
          "id": "1a448531a6b8f301a08538fc4a1d7c9c06a02e8a",
          "message": "feat: warn on match with identical arms (#562)",
          "timestamp": "2026-05-31T23:51:46+02:00",
          "url": "https://github.com/ivov/lisette/commit/1a448531a6b8f301a08538fc4a1d7c9c06a02e8a"
        },
        "date": 1780264306000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 90103
          },
          {
            "name": "emit",
            "unit": "lines",
            "value": 27470
          },
          {
            "name": "semantics",
            "unit": "lines",
            "value": 26093
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
            "value": 4429
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
          "id": "b2a14786f669de658d0456966ca0553076e314e0",
          "message": "chore: release v0.3.2 (#638)",
          "timestamp": "2026-06-07T22:52:48+02:00",
          "url": "https://github.com/ivov/lisette/commit/b2a14786f669de658d0456966ca0553076e314e0"
        },
        "date": 1780865568000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 93011
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
            "value": 2737
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
          "id": "15bba7798df2d33ee79032062e1e75f1cc86acd6",
          "message": "chore: release v0.4.1 (#713)",
          "timestamp": "2026-06-14T23:11:21+02:00",
          "url": "https://github.com/ivov/lisette/commit/15bba7798df2d33ee79032062e1e75f1cc86acd6"
        },
        "date": 1781471481000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 96593
          },
          {
            "name": "semantics",
            "unit": "lines",
            "value": 31251
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
            "value": 5054
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
          "id": "1c39c0a576348c920da097a54ff46cb2c2a7d29f",
          "message": "fix: preserve value-less const declarations when formatting (#817)",
          "timestamp": "2026-06-21T22:34:33+02:00",
          "url": "https://github.com/ivov/lisette/commit/1c39c0a576348c920da097a54ff46cb2c2a7d29f"
        },
        "date": 1782074073000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 104626
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
            "value": 8438
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
            "value": 813
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
          "id": "7ad0238151fc559ee18aeb15f3a27dbfb523b9e8",
          "message": "chore: release v0.6.0 (#869)",
          "timestamp": "2026-06-28T19:05:31+02:00",
          "url": "https://github.com/ivov/lisette/commit/7ad0238151fc559ee18aeb15f3a27dbfb523b9e8"
        },
        "date": 1782666331000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 106649
          },
          {
            "name": "emit",
            "unit": "lines",
            "value": 27257
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
          "id": "120eb8e8a987911f19ab20fc0b5177ade32d5a8f",
          "message": "feat: autofix seven more lints (#932)",
          "timestamp": "2026-06-30T23:45:55+02:00",
          "url": "https://github.com/ivov/lisette/commit/120eb8e8a987911f19ab20fc0b5177ade32d5a8f"
        },
        "date": 1782855955000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 108281
          },
          {
            "name": "emit",
            "unit": "lines",
            "value": 27404
          },
          {
            "name": "semantics",
            "unit": "lines",
            "value": 21645
          },
          {
            "name": "passes",
            "unit": "lines",
            "value": 14996
          },
          {
            "name": "syntax",
            "unit": "lines",
            "value": 12545
          },
          {
            "name": "cli",
            "unit": "lines",
            "value": 10059
          },
          {
            "name": "diagnostics",
            "unit": "lines",
            "value": 5852
          },
          {
            "name": "lsp",
            "unit": "lines",
            "value": 4310
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
            "value": 866
          }
        ]
      },
      {
        "commit": {
          "id": "6b9ce9dbe7043eca38d0b59eedefeadb43e2924c",
          "message": "refactor: skip `Result` for Go-interop `?` in `try` blocks (#965)",
          "timestamp": "2026-07-05T22:55:47+02:00",
          "url": "https://github.com/ivov/lisette/commit/6b9ce9dbe7043eca38d0b59eedefeadb43e2924c"
        },
        "date": 1783284947000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "unit": "lines",
            "value": 110357
          },
          {
            "name": "emit",
            "unit": "lines",
            "value": 27452
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
          "id": "48c68835ffdaa11621c2f47ff8486db7f2eb57ef",
          "message": "fix: reject missing transitive generic bounds (#1039)",
          "timestamp": "2026-07-18T15:41:42+02:00",
          "tree_id": "1c45264bae6311797be20fdff13c01fbf0956c82",
          "url": "https://github.com/ivov/lisette/commit/48c68835ffdaa11621c2f47ff8486db7f2eb57ef"
        },
        "date": 1784382125748,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 115695,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29969,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 23574,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16621,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13206,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10137,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6300,
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
          "id": "a1aa9da4304a0dae70768d52fd9cfb7ad3f61a6b",
          "message": "feat: `lis check --deny warnings` (#1042)",
          "timestamp": "2026-07-18T16:07:02+02:00",
          "tree_id": "0eef8824cb8c36d0c2f46c45127c8dcb279ecc8e",
          "url": "https://github.com/ivov/lisette/commit/a1aa9da4304a0dae70768d52fd9cfb7ad3f61a6b"
        },
        "date": 1784383645168,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 115752,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29969,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 23574,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16621,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13206,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10194,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6300,
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
          "id": "523fbbb417454b4eb743449cd8de3e5067672645",
          "message": "fix: make `#[json]` struct field capitalization consistent (#1043)",
          "timestamp": "2026-07-18T16:35:36+02:00",
          "tree_id": "b2006c622635052936d020b5c8f22b74475cba42",
          "url": "https://github.com/ivov/lisette/commit/523fbbb417454b4eb743449cd8de3e5067672645"
        },
        "date": 1784385362422,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 115752,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29969,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 23574,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16621,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13206,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10194,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6300,
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
          "id": "0fb4dce2a39d68768084219e895995003b27e4bd",
          "message": "fix: allow suppressing unused symbol lints (#1044)",
          "timestamp": "2026-07-18T17:46:43+02:00",
          "tree_id": "83152e9d13b25bb1f5932cebab29cd6622e6e676",
          "url": "https://github.com/ivov/lisette/commit/0fb4dce2a39d68768084219e895995003b27e4bd"
        },
        "date": 1784389624331,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 115805,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 29969,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 23574,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16672,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13207,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10194,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6300,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4415,
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
          "id": "7745077902de90cfde6d1b91cb8c526befd69ef7",
          "message": "fix: interface satisfaction for generic `Result` methods (#1046)",
          "timestamp": "2026-07-18T20:15:49+02:00",
          "tree_id": "58fff6c53069c0d8f28ef0ac62ccd84998aa6ada",
          "url": "https://github.com/ivov/lisette/commit/7745077902de90cfde6d1b91cb8c526befd69ef7"
        },
        "date": 1784398570596,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 115938,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30097,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 23579,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16672,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13207,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10194,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6300,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4415,
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
          "id": "ab72f1b23b76fdc0aa6bb12ed3e0e874e05a8940",
          "message": "fix: tighten `reference_aliases_sibling` lint (#1050)",
          "timestamp": "2026-07-18T21:30:32+02:00",
          "tree_id": "3d132988e9a2a51c9010a3c267db4b7369ecf18f",
          "url": "https://github.com/ivov/lisette/commit/ab72f1b23b76fdc0aa6bb12ed3e0e874e05a8940"
        },
        "date": 1784403053364,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 115959,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30097,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 23601,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16672,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13207,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10194,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6299,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4415,
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
          "id": "26c92e7ca027f18d143ab8fe572e75a32cad00c6",
          "message": "perf: drop raw Go-stdlib cache before inference (#1052)",
          "timestamp": "2026-07-18T23:42:47+02:00",
          "tree_id": "0663aa8e4536b18720168d91646402dfa3fcf735",
          "url": "https://github.com/ivov/lisette/commit/26c92e7ca027f18d143ab8fe572e75a32cad00c6"
        },
        "date": 1784410989327,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 115963,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30097,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 23605,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16672,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13207,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10194,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6299,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4415,
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
          "id": "50774c79254d599dd085f2369ca4c7fabb3e16d6",
          "message": "fix: reject uninferred bounded type args in construction (#1053)",
          "timestamp": "2026-07-18T23:43:33+02:00",
          "tree_id": "376499a3ef82c745c34ec9ea7e53b17178602778",
          "url": "https://github.com/ivov/lisette/commit/50774c79254d599dd085f2369ca4c7fabb3e16d6"
        },
        "date": 1784411034849,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 116150,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30097,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 23765,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16683,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13207,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10194,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6315,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4415,
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
          "id": "443df9fdd3a6ceccec212f531715e0be55737a4d",
          "message": "fix: reject generic function used as a value with uninferable type (#1060)",
          "timestamp": "2026-07-19T12:18:08+02:00",
          "tree_id": "c37f807daa78d0d4ee12f0b2974213d5da7009a8",
          "url": "https://github.com/ivov/lisette/commit/443df9fdd3a6ceccec212f531715e0be55737a4d"
        },
        "date": 1784456308797,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 116200,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30097,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 23794,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16683,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13207,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10194,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6336,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4415,
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
          "id": "4171e6398f9e14a3ff758a7da0d1939f4c409862",
          "message": "feat: lint for discarding a unit expression (#1064)",
          "timestamp": "2026-07-19T13:22:46+02:00",
          "tree_id": "cf7a08ac57065112474460d56de52081825e68b1",
          "url": "https://github.com/ivov/lisette/commit/4171e6398f9e14a3ff758a7da0d1939f4c409862"
        },
        "date": 1784460187308,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 116258,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30097,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 23794,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16733,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13207,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10194,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6344,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4415,
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
          "id": "2856c2e2dc1ed752b8348be8f1ae3a60cd387938",
          "message": "fix: collision between built-in `Array` and third-party `Array` (#1065)",
          "timestamp": "2026-07-19T13:31:57+02:00",
          "tree_id": "77ebf03c6593d2cf08392c6cb5adb5b9c2f0bd83",
          "url": "https://github.com/ivov/lisette/commit/2856c2e2dc1ed752b8348be8f1ae3a60cd387938"
        },
        "date": 1784460740644,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 116262,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30097,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 23794,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16733,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13207,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10194,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6344,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4415,
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
            "value": 6050,
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
          "id": "327c8b032f6570431f06df57394c4203a31e159f",
          "message": "refactor: reword `reference_aliases_sibling` helptext (#1063)",
          "timestamp": "2026-07-19T14:12:48+02:00",
          "tree_id": "1c316591dd4b05699db2f55fbef5a577e6622b00",
          "url": "https://github.com/ivov/lisette/commit/327c8b032f6570431f06df57394c4203a31e159f"
        },
        "date": 1784463190016,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 116264,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30097,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 23794,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16733,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13207,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10194,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6346,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4415,
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
            "value": 6050,
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
          "id": "b8e8d6c5323425b2aa8f874a58da62ee2927d3cf",
          "message": "fix: enforce generic bounds on indirect uses of bounded types (#1066)",
          "timestamp": "2026-07-19T15:03:56+02:00",
          "tree_id": "166719eac551128064f76dfe61520f5c063360f8",
          "url": "https://github.com/ivov/lisette/commit/b8e8d6c5323425b2aa8f874a58da62ee2927d3cf"
        },
        "date": 1784466259761,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 116475,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30097,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 23977,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16743,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13207,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10194,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6364,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4415,
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
            "value": 6050,
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
          "id": "21f30083640b22ea3a924daa9269a01e4ed08d35",
          "message": "chore: release v0.9.0 (#1006)",
          "timestamp": "2026-07-19T15:19:23+02:00",
          "tree_id": "ad6718da1a933f5491dfc686af7f6499d1eebc53",
          "url": "https://github.com/ivov/lisette/commit/21f30083640b22ea3a924daa9269a01e4ed08d35"
        },
        "date": 1784467184587,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 116475,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30097,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 23977,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16743,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13207,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10194,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6364,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4415,
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
            "value": 6050,
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
          "id": "c2f92e55641b849804c7c28e4299f3c5ed0dce8b",
          "message": "fix: refresh LSP diagnostics in other open files post-edit (#1070)",
          "timestamp": "2026-07-19T18:09:15+02:00",
          "tree_id": "d9d6fc02f140c2b24fbd031625e18716447887b7",
          "url": "https://github.com/ivov/lisette/commit/c2f92e55641b849804c7c28e4299f3c5ed0dce8b"
        },
        "date": 1784477378023,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 116491,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30097,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 23977,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16743,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13207,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10194,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6364,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4431,
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
            "value": 6050,
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
          "id": "5a3f8554acf570c21727eb7a346db947a19a0a48",
          "message": "refactor: unify generic bound handling (#1072)",
          "timestamp": "2026-07-19T18:41:26+02:00",
          "tree_id": "d705563ac226b00caa99bd5f33a6f097789e2762",
          "url": "https://github.com/ivov/lisette/commit/5a3f8554acf570c21727eb7a346db947a19a0a48"
        },
        "date": 1784479306446,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 116584,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30097,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24110,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16703,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13207,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10194,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6364,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4431,
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
            "value": 6050,
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
          "id": "4e2c006122d0c979b5e152b6be8d506bc4ac5f4d",
          "message": "fix: reject variable shadowing imported module (#1073)",
          "timestamp": "2026-07-19T18:45:56+02:00",
          "tree_id": "76766936e54232cee4768a694659a075e58d040a",
          "url": "https://github.com/ivov/lisette/commit/4e2c006122d0c979b5e152b6be8d506bc4ac5f4d"
        },
        "date": 1784479577213,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 116594,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30097,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24124,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16703,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13207,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10194,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6360,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4431,
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
            "value": 6050,
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
          "id": "4564597d46c4198a8a0d36366652a11d25cc29b0",
          "message": "fix: resolve forward-referenced type aliases (#1074)",
          "timestamp": "2026-07-19T19:57:55+02:00",
          "tree_id": "5609a9a1086a211c2614a2022c7332c5e0c65988",
          "url": "https://github.com/ivov/lisette/commit/4564597d46c4198a8a0d36366652a11d25cc29b0"
        },
        "date": 1784483898776,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 116726,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30097,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24256,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16703,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13207,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10194,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6360,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4431,
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
            "value": 6050,
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
          "id": "185ff660fdf7f5ead1186e9b370e022870dcc8f4",
          "message": "feat: `lis check` for orphan modules (#1075)",
          "timestamp": "2026-07-19T20:46:42+02:00",
          "tree_id": "0baef753df40403b508bddd3c71e7ecab7defd45",
          "url": "https://github.com/ivov/lisette/commit/185ff660fdf7f5ead1186e9b370e022870dcc8f4"
        },
        "date": 1784486825833,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 116813,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30097,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24304,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16706,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13207,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10215,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6375,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4431,
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
            "value": 6050,
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
          "id": "96286bc3f55e5399f45bca020e60a519902218c2",
          "message": "refactor: simplify select channel handling (#1077)",
          "timestamp": "2026-07-19T22:30:22+02:00",
          "tree_id": "5376361219d4cd304ba17b4e286573a00b2555f3",
          "url": "https://github.com/ivov/lisette/commit/96286bc3f55e5399f45bca020e60a519902218c2"
        },
        "date": 1784493044286,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 116775,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30076,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24228,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16706,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13266,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10215,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6375,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4431,
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
            "value": 6050,
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
          "id": "4ac94a265c77577f7577a4e094900e6e12a573be",
          "message": "refactor: simplify cache persistence (#1078)",
          "timestamp": "2026-07-19T22:44:24+02:00",
          "tree_id": "5395855b06e3e7bf104f8a9f85f1d50912e1e532",
          "url": "https://github.com/ivov/lisette/commit/4ac94a265c77577f7577a4e094900e6e12a573be"
        },
        "date": 1784493886787,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 116740,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30076,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24193,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16706,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13266,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10215,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6375,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4431,
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
            "value": 6050,
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
          "id": "1cccc2f40610970d972d717af4c2dd7ac356abd6",
          "message": "refactor: drop getters that mirror public fields (#1079)",
          "timestamp": "2026-07-20T17:54:00+02:00",
          "tree_id": "21444c1cc295a65c433c130d5e2048dce5409b49",
          "url": "https://github.com/ivov/lisette/commit/1cccc2f40610970d972d717af4c2dd7ac356abd6"
        },
        "date": 1784562862400,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 116725,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30077,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24193,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16706,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13254,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10215,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6375,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4427,
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
            "value": 6050,
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
          "id": "b1ea9024e797b49b49b60d0286b8a1aa94ac606e",
          "message": "feat: bind generics bounded by embedding interfaces or error (#1080)",
          "timestamp": "2026-07-20T19:06:09+02:00",
          "tree_id": "5e37f8b3e8b3a7f94aa9a571784ae214e5f0a015",
          "url": "https://github.com/ivov/lisette/commit/b1ea9024e797b49b49b60d0286b8a1aa94ac606e"
        },
        "date": 1784567199452,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 116759,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30077,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24193,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16706,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13254,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10215,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6375,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4427,
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
            "value": 6084,
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
          "id": "74d1e5a5293742e9553d15aa73a5cd4eeb42fd42",
          "message": "fix: enforce generic bounds on nested return types (#1081)",
          "timestamp": "2026-07-20T19:56:37+02:00",
          "tree_id": "6c2e91c415fbb3a103b51a5168f2c9c79ae35a02",
          "url": "https://github.com/ivov/lisette/commit/74d1e5a5293742e9553d15aa73a5cd4eeb42fd42"
        },
        "date": 1784570225993,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 116805,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30077,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24237,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16708,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13254,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10215,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6375,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4427,
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
            "value": 6084,
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
          "id": "f0b34c6bc71572efe75b76e60c57273b08b3d3b6",
          "message": "refactor: model project kind and shared project layout (#1083)",
          "timestamp": "2026-07-20T22:09:05+02:00",
          "tree_id": "28f2f31edcabfd77a7ad7c27918fe1323121e96f",
          "url": "https://github.com/ivov/lisette/commit/f0b34c6bc71572efe75b76e60c57273b08b3d3b6"
        },
        "date": 1784578167848,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 116873,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30098,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24250,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16708,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13254,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10240,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6375,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4428,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2812,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "03d598eaa80817925e01ebaeaf05fbc5d9a507ed",
          "message": "fix: shadowed variable in `else` leaking past diverging `if` (#1084)",
          "timestamp": "2026-07-20T22:31:50+02:00",
          "tree_id": "83a6dd4c5d3b1a5fe56aeb28e1919d8ff09e67fd",
          "url": "https://github.com/ivov/lisette/commit/03d598eaa80817925e01ebaeaf05fbc5d9a507ed"
        },
        "date": 1784579533937,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 116875,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30100,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24250,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16708,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13254,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10240,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6375,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4428,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2812,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "ce9d2751ce037166eee03eb7e06da803f5767f7a",
          "message": "fix: preserve tuple type aliases at use sites (#1082)",
          "timestamp": "2026-07-20T22:42:03+02:00",
          "tree_id": "714811dc68b727ff4a206cee02789eb2655cb7f6",
          "url": "https://github.com/ivov/lisette/commit/ce9d2751ce037166eee03eb7e06da803f5767f7a"
        },
        "date": 1784580144421,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 116884,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30100,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24251,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16708,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13262,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10240,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6375,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4428,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2812,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "0e8dca4f0bf5f6fd6f705e4f819c3be53c8166d7",
          "message": "fix: false positives in lints (#1085)",
          "timestamp": "2026-07-20T22:42:14+02:00",
          "tree_id": "69e74bd45a0de789f2eee271681afe5f7f424830",
          "url": "https://github.com/ivov/lisette/commit/0e8dca4f0bf5f6fd6f705e4f819c3be53c8166d7"
        },
        "date": 1784580157443,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117007,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30100,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24251,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16831,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13262,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10240,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6375,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4428,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2812,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "af6c3ee226783a820884a715a80001c2c46a8a2b",
          "message": "ci: enforce block-style snapshot descriptions (#1086)",
          "timestamp": "2026-07-20T23:02:37+02:00",
          "tree_id": "c15b48af4e6cb1238d4fa509149d54e95f145663",
          "url": "https://github.com/ivov/lisette/commit/af6c3ee226783a820884a715a80001c2c46a8a2b"
        },
        "date": 1784581377741,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117007,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30100,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24251,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16831,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13262,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10240,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6375,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4428,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2812,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "43ded482f2760b9bee83e797382e241c9811016c",
          "message": "refactor: suggest `error` for `Err` in type position (#1088)",
          "timestamp": "2026-07-21T23:45:32+02:00",
          "tree_id": "518c8ae8b4377ca5e145437900c4beb6ce2b0bf2",
          "url": "https://github.com/ivov/lisette/commit/43ded482f2760b9bee83e797382e241c9811016c"
        },
        "date": 1784670353366,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117010,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30100,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24254,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16831,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13262,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10240,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6375,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4428,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2812,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "95ba74805c184eaa30416f7ecbfa13ce907b1bc6",
          "message": "fix: parse `fn` types in tuple struct fields (#1089)",
          "timestamp": "2026-07-22T00:06:41+02:00",
          "tree_id": "2cd7e48ba834c4d64551f8d8c340c01da738603a",
          "url": "https://github.com/ivov/lisette/commit/95ba74805c184eaa30416f7ecbfa13ce907b1bc6"
        },
        "date": 1784671757990,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117016,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30100,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24254,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16831,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13268,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10240,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6375,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4428,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2812,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "be4a595820856662b87239761b03bea89bf39d60",
          "message": "fix: stack overflow on circular aliases with growing arguments (#1090)",
          "timestamp": "2026-07-22T00:45:43+02:00",
          "tree_id": "e17b2da9ffe3396ee208799d3121b7b456fc92ee",
          "url": "https://github.com/ivov/lisette/commit/be4a595820856662b87239761b03bea89bf39d60"
        },
        "date": 1784673965447,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117039,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30100,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24258,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16850,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13268,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10240,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6375,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4428,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2812,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "c0ed9c46d96b921cac40c94f52edb46d3fa93491",
          "message": "refactor: extract check-handler compile and report helpers (#1091)",
          "timestamp": "2026-07-22T20:03:51+02:00",
          "tree_id": "66d1b6fc5592216b33c9c11832fbf46f99ac54ca",
          "url": "https://github.com/ivov/lisette/commit/c0ed9c46d96b921cac40c94f52edb46d3fa93491"
        },
        "date": 1784743459953,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117082,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30100,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24258,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16850,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13268,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10283,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6375,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4428,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2812,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "45ed646e1ba501a50145aea77a7b10392d48e4f1",
          "message": "feat: interface satisfaction by emitted Go name (#1092)",
          "timestamp": "2026-07-22T21:55:57+02:00",
          "tree_id": "05eb79d0bdb8f431b164ca22c9b922dec960a70c",
          "url": "https://github.com/ivov/lisette/commit/45ed646e1ba501a50145aea77a7b10392d48e4f1"
        },
        "date": 1784750179639,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117499,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30103,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24590,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16850,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13350,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10283,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6375,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4428,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2812,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "c0d5d45efe452cce6796647ee8be8c91eff7957c",
          "message": "feat: flag safe-to-rename interface-satisfying methods (#1093)",
          "timestamp": "2026-07-22T22:44:32+02:00",
          "tree_id": "91e3a69d769d9bdc01e362b39d5c472e37e389c1",
          "url": "https://github.com/ivov/lisette/commit/c0d5d45efe452cce6796647ee8be8c91eff7957c"
        },
        "date": 1784753100273,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117522,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30103,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24608,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16850,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13355,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10283,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6375,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4428,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2812,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "1acd3408bbd3bb585d8707872318528bb3a7f217",
          "message": "refactor: tighten visibility and remove dead code (#1095)",
          "timestamp": "2026-07-23T00:06:38+02:00",
          "tree_id": "a88ca24b0662d915d4b382a4c52664b0625f8858",
          "url": "https://github.com/ivov/lisette/commit/1acd3408bbd3bb585d8707872318528bb3a7f217"
        },
        "date": 1784758021684,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117309,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30091,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24506,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16850,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13283,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10283,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6363,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4425,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2800,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "a52895d4ebc28abcc8f38ee3f6ea660ee1fbdb7b",
          "message": "chore: sample month-end commits in LoC backfill (#1096)",
          "timestamp": "2026-07-23T20:15:04+02:00",
          "tree_id": "ab27b1536f269edab9f475a01c8c8a504a47fcf1",
          "url": "https://github.com/ivov/lisette/commit/a52895d4ebc28abcc8f38ee3f6ea660ee1fbdb7b"
        },
        "date": 1784830527063,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117309,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30091,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24506,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16850,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13283,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10283,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6363,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4425,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2800,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "afb405e067f8d45b02b9f361217cbf023875c444",
          "message": "feat: library projects (#1094)",
          "timestamp": "2026-07-23T21:29:00+02:00",
          "tree_id": "e8f613d0f2083daf8a7f884b370dc52021c483c1",
          "url": "https://github.com/ivov/lisette/commit/afb405e067f8d45b02b9f361217cbf023875c444"
        },
        "date": 1784834960673,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117626,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30097,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24527,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16850,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13283,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10560,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6363,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4438,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2800,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "8fef96b3111d59cecb57f8178142470a1195a8e8",
          "message": "refactor: simplify inference flows (#1097)",
          "timestamp": "2026-07-23T21:41:32+02:00",
          "tree_id": "177ff827ad7faf0a0105984a54bf25f6b19a1108",
          "url": "https://github.com/ivov/lisette/commit/8fef96b3111d59cecb57f8178142470a1195a8e8"
        },
        "date": 1784835714698,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117537,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30097,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24375,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16913,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13283,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10560,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6363,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4438,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2800,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "ab717310ebe17e3fac04a3aac82e6a62db2ac0b9",
          "message": "fix: emit idiomatic Go casing for constants and private names (#1098)",
          "timestamp": "2026-07-23T22:06:10+02:00",
          "tree_id": "d20720d4763f034f384f9e44961a2384530de851",
          "url": "https://github.com/ivov/lisette/commit/ab717310ebe17e3fac04a3aac82e6a62db2ac0b9"
        },
        "date": 1784837193929,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117627,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30149,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24375,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16913,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13321,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10560,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6363,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4438,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2800,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "02b13a0ea7a23bbb1824e1435084d36ac1d23393",
          "message": "refactor: remodel emit invariants (#1099)",
          "timestamp": "2026-07-23T22:40:50+02:00",
          "tree_id": "818b8c1e46a84af04d03686b05c47e0d91ec727b",
          "url": "https://github.com/ivov/lisette/commit/02b13a0ea7a23bbb1824e1435084d36ac1d23393"
        },
        "date": 1784839274799,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117565,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30087,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24375,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16913,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13321,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10560,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6363,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4438,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2800,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "5135b35807bbc210bc211a9626391a767943afa8",
          "message": "refactor: simplify syntax representations (#1100)",
          "timestamp": "2026-07-23T23:00:30+02:00",
          "tree_id": "496295f7104f7990a7808417056035041754c56e",
          "url": "https://github.com/ivov/lisette/commit/5135b35807bbc210bc211a9626391a767943afa8"
        },
        "date": 1784840454085,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117663,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30114,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24442,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16914,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13334,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10558,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6363,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4439,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2791,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "077367874e1825d574d760409f2344275419397d",
          "message": "refactor: emit idiomatic Go for chains, errors, and key-only loops (#1103)",
          "timestamp": "2026-07-24T18:38:10+02:00",
          "tree_id": "dd70539a777dbe6b26e9d56d5fd8b6f70f78e55f",
          "url": "https://github.com/ivov/lisette/commit/077367874e1825d574d760409f2344275419397d"
        },
        "date": 1784911117026,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117760,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30211,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24442,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16914,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13334,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10558,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6363,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4439,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2791,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "852d2102e606f2c779199137d8ee28a8ab1cb382",
          "message": "refactor: polish arithmetic, type name, and interface diagnostics (#1104)",
          "timestamp": "2026-07-24T18:55:03+02:00",
          "tree_id": "d8cf82bfaa2fd10f4bcd33a62189a2acb9168177",
          "url": "https://github.com/ivov/lisette/commit/852d2102e606f2c779199137d8ee28a8ab1cb382"
        },
        "date": 1784912125481,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117879,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30211,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24500,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16914,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13358,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10558,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6400,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4439,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2791,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "5a0fcccdb2a92e8177ad1fc378f0b57ee7f24256",
          "message": "fix: bound AST depth for long operator chains (#1105)",
          "timestamp": "2026-07-24T20:43:01+02:00",
          "tree_id": "13a43085cff878fcc8aa9349fd8771605df09468",
          "url": "https://github.com/ivov/lisette/commit/5a0fcccdb2a92e8177ad1fc378f0b57ee7f24256"
        },
        "date": 1784918603209,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117888,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30211,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24500,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16914,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13367,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10558,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6400,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4439,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2791,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "1ac30e21c58d2d0b8477dd8ae76e53a472ec76ab",
          "message": "feat: add LSP restart command and label-first diagnostics (#1106)",
          "timestamp": "2026-07-24T21:05:57+02:00",
          "tree_id": "5414cb1f92ac97c5c1b1325126004798d9e9e9b8",
          "url": "https://github.com/ivov/lisette/commit/1ac30e21c58d2d0b8477dd8ae76e53a472ec76ab"
        },
        "date": 1784919983953,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117896,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30211,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24500,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16914,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13367,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10558,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6405,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4442,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2791,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "bb5f6b47575d96972d6158a810e1d279000a4b70",
          "message": "ci: seed fuzz targets and cover missing operators (#1107)",
          "timestamp": "2026-07-24T22:00:10+02:00",
          "tree_id": "2d6f5377f73ea08c3745c7b12a8ae1ce870781d2",
          "url": "https://github.com/ivov/lisette/commit/bb5f6b47575d96972d6158a810e1d279000a4b70"
        },
        "date": 1784923235396,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117896,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30211,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24500,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16914,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13367,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10558,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6405,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4442,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2791,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "fc853fbb69ade05b7548f367e0c4341b25ed874d",
          "message": "refactor: restyle test report (#1108)",
          "timestamp": "2026-07-24T22:00:19+02:00",
          "tree_id": "c47cb1b3b13c33ddae5e100b923565d8fe1ffc49",
          "url": "https://github.com/ivov/lisette/commit/fc853fbb69ade05b7548f367e0c4341b25ed874d"
        },
        "date": 1784923245529,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117896,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30211,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24501,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 16914,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13367,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10550,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6412,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4442,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2791,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "45507087b5ac0f4b4681a9f4d2ecb5f593aff66a",
          "message": "refactor: encode more invariants in type system (#1109)",
          "timestamp": "2026-07-25T02:55:55+02:00",
          "tree_id": "ac095db461343b87fcadf54502777bb01ae73a29",
          "url": "https://github.com/ivov/lisette/commit/45507087b5ac0f4b4681a9f4d2ecb5f593aff66a"
        },
        "date": 1784940977086,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117436,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30046,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24035,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17081,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13394,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10556,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6411,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4419,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2786,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "2319347682d8fea943b0e516b182ee0322e05bad",
          "message": "refactor: reduce redundant representation state (#1113)",
          "timestamp": "2026-07-25T13:59:21+02:00",
          "tree_id": "fba2c2201f6b4a0264c3f7c93ab165fc295e7a5c",
          "url": "https://github.com/ivov/lisette/commit/2319347682d8fea943b0e516b182ee0322e05bad"
        },
        "date": 1784980786986,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117711,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30073,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24020,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17139,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13587,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10548,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6411,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4420,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2805,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "ef984d8494ff237248d95f968b28eb6019870719",
          "message": "fix: reject embedding a non-interface in an interface (#1114)",
          "timestamp": "2026-07-25T15:06:55+02:00",
          "tree_id": "f87efe3352dadfe2871ced16fb976d5bbde24bc7",
          "url": "https://github.com/ivov/lisette/commit/ef984d8494ff237248d95f968b28eb6019870719"
        },
        "date": 1784984835648,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117754,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30073,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24049,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17139,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13591,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10548,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6421,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4420,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2805,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "e0b487998678b46d2a771d1f49afa12e371ff461",
          "message": "fix: accept Go function values in prelude combinators (#1115)",
          "timestamp": "2026-07-25T15:18:28+02:00",
          "tree_id": "b2111c5eb4b9fbffe08a501def10b3246567a5f5",
          "url": "https://github.com/ivov/lisette/commit/e0b487998678b46d2a771d1f49afa12e371ff461"
        },
        "date": 1784985530080,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117747,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30066,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24049,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17139,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13591,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10548,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6421,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4420,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2805,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "b0f81f109cf8db5b069533155672d620b108da4b",
          "message": "fix: check function types invariantly at param and return (#1116)",
          "timestamp": "2026-07-25T15:43:21+02:00",
          "tree_id": "ec3dc301fae35669b300031113dfd3758ca37028",
          "url": "https://github.com/ivov/lisette/commit/b0f81f109cf8db5b069533155672d620b108da4b"
        },
        "date": 1784987022772,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117831,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30066,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24104,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17168,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13591,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10548,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6421,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4420,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2805,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "93bb8985386dd0bf4e7d758d0b9fe146fdaaa8a6",
          "message": "fix: rebuild arrays and tuples whose elements widen to interfaces (#1117)",
          "timestamp": "2026-07-25T15:49:55+02:00",
          "tree_id": "4d5ae6721c215f5dae47c5a23a5b90edf6fc9e8e",
          "url": "https://github.com/ivov/lisette/commit/93bb8985386dd0bf4e7d758d0b9fe146fdaaa8a6"
        },
        "date": 1784987414694,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117930,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30165,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24104,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17168,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13591,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10548,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6421,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4420,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2805,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "ab6219925c6ed20c2ce21032191a283ff52a5c36",
          "message": "fix: never split a lambda param list to fit a long call (#1118)",
          "timestamp": "2026-07-25T16:19:52+02:00",
          "tree_id": "00dfbfc2d0af96e840b938b80fa5ed7ea41bec37",
          "url": "https://github.com/ivov/lisette/commit/ab6219925c6ed20c2ce21032191a283ff52a5c36"
        },
        "date": 1784989214537,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117915,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30165,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24104,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17168,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13591,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10548,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6421,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4420,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2790,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "6f46024ff9e886cade8f9f48cf0bc41be6609404",
          "message": "refactor: remove trivia duplication and flatten select arms (#1120)",
          "timestamp": "2026-07-25T17:04:06+02:00",
          "tree_id": "77f960b38ae68931d8ce0e86e0834294b83659fa",
          "url": "https://github.com/ivov/lisette/commit/6f46024ff9e886cade8f9f48cf0bc41be6609404"
        },
        "date": 1784992007512,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117869,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30167,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24110,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17168,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13556,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10548,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6421,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4420,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2771,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "e9a613b5e32f0c4494abae36defd81b76f41751d",
          "message": "refactor: unify formatter trivia handling (#1121)",
          "timestamp": "2026-07-25T17:34:40+02:00",
          "tree_id": "28ee450c2e59650873831412bec70d578c4df739",
          "url": "https://github.com/ivov/lisette/commit/e9a613b5e32f0c4494abae36defd81b76f41751d"
        },
        "date": 1784993701325,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117862,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30167,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24110,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17168,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13556,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10548,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6421,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4420,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2764,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "68e7c4e2e5d88656ba42021e9ca579161aa72569",
          "message": "chore: sample LoC backfill at week and month ends in UTC (#1122)",
          "timestamp": "2026-07-25T17:52:35+02:00",
          "tree_id": "a4f2b60026bb068b621818d7d72dacda8ede4a23",
          "url": "https://github.com/ivov/lisette/commit/68e7c4e2e5d88656ba42021e9ca579161aa72569"
        },
        "date": 1784994776931,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117862,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30167,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24110,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17168,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13556,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10548,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6421,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4420,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2764,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "26aa2b7d705a4aa6f4839ac15dd8652abc483722",
          "message": "refactor: simplify bindgen (#1123)",
          "timestamp": "2026-07-25T19:11:35+02:00",
          "tree_id": "d41e0a1416f6cb9e15d6fac68ea031b366e22132",
          "url": "https://github.com/ivov/lisette/commit/26aa2b7d705a4aa6f4839ac15dd8652abc483722"
        },
        "date": 1784999519904,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117862,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30167,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24110,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17168,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13556,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10548,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6421,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4420,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2764,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "baed5fdd57917192e66f416b2a736999feb24aeb",
          "message": "refactor: simplify diagnostic passes (#1124)",
          "timestamp": "2026-07-25T19:22:41+02:00",
          "tree_id": "e3bb4a933f0d692eb829438e5ce8b6bc28259300",
          "url": "https://github.com/ivov/lisette/commit/baed5fdd57917192e66f416b2a736999feb24aeb"
        },
        "date": 1785000184135,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117741,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30167,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24113,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17044,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13556,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10548,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6421,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4420,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2764,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "2a085e3257e83de8cc6a3d8bae70bb57b6b8d6d0",
          "message": "refactor: simplify LSP's state model (#1125)",
          "timestamp": "2026-07-25T19:59:57+02:00",
          "tree_id": "f7f0586c3ef092b01c4713c115d389b8c76f188b",
          "url": "https://github.com/ivov/lisette/commit/2a085e3257e83de8cc6a3d8bae70bb57b6b8d6d0"
        },
        "date": 1785002418764,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117754,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30167,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24113,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17044,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13556,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10548,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6421,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4433,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2764,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "42ef2b7cfa61a9656f44248399487589f7bed432",
          "message": "refactor: simplify diagnostics collection (#1126)",
          "timestamp": "2026-07-25T20:10:30+02:00",
          "tree_id": "915a5c7885d6f926eb8ac0a35dd79d2efd75f72e",
          "url": "https://github.com/ivov/lisette/commit/42ef2b7cfa61a9656f44248399487589f7bed432"
        },
        "date": 1785003053446,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 117948,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30170,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24139,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17068,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13557,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10535,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6555,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4452,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2764,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "2479b51205bd9a9a3fa228cdc06c321a56e42a65",
          "message": "refactor: enforce build sequencing in CLI (#1127)",
          "timestamp": "2026-07-25T20:53:39+02:00",
          "tree_id": "8c8e15f361e403ba25654349e4b7a9f8c1400550",
          "url": "https://github.com/ivov/lisette/commit/2479b51205bd9a9a3fa228cdc06c321a56e42a65"
        },
        "date": 1785005638437,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 118223,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30170,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24139,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17068,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13557,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10810,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6555,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4452,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2764,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "575af7eee3ce44e4294a66e0d238ede534e518d1",
          "message": "refactor: model more semantics state with types (#1128)",
          "timestamp": "2026-07-25T23:53:54+02:00",
          "tree_id": "6f0f2f5c0375bb643a3335d1756040b4c97eeee4",
          "url": "https://github.com/ivov/lisette/commit/575af7eee3ce44e4294a66e0d238ede534e518d1"
        },
        "date": 1785016456040,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 118489,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30170,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24397,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17077,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13557,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10810,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6555,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4451,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2764,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "23eb82c5fb1109ff49be379a00aef719cf498304",
          "message": "perf: scan project sources once per check (#1129)",
          "timestamp": "2026-07-26T01:12:07+02:00",
          "tree_id": "53e47ddbb53d6371393301ff14e5a1ef1b9ac1a8",
          "url": "https://github.com/ivov/lisette/commit/23eb82c5fb1109ff49be379a00aef719cf498304"
        },
        "date": 1785021148584,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 118528,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30170,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24397,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17077,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13557,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10849,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6555,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4451,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2764,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "abc1cf50478eb1ca285dbcc7a63facc185d404b1",
          "message": "refactor: replace raw LSP test positions with cursor markers (#1130)",
          "timestamp": "2026-07-26T14:27:46+02:00",
          "tree_id": "fe18f0f739b98863f72354ced1852709f27b29e6",
          "url": "https://github.com/ivov/lisette/commit/abc1cf50478eb1ca285dbcc7a63facc185d404b1"
        },
        "date": 1785068886777,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 118528,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30170,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24397,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17077,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13557,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10849,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6555,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4451,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2764,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "0b5fdfda8a2f7af1b26cad579fed25505338cc5d",
          "message": "refactor: remove and forbid `too_many_arguments` exemptions (#1131)",
          "timestamp": "2026-07-26T14:34:44+02:00",
          "tree_id": "c11ac74f8c07a1f90ad48bdc4e80030d26602075",
          "url": "https://github.com/ivov/lisette/commit/0b5fdfda8a2f7af1b26cad579fed25505338cc5d"
        },
        "date": 1785069306847,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 118464,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30192,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24384,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17027,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13559,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 10840,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6546,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4444,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2764,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "f31d29db48f8cd39ba7ddfc189d2dded7b4f5bd2",
          "message": "feat: external tests (#1119)",
          "timestamp": "2026-07-26T14:44:40+02:00",
          "tree_id": "3ea542285a5ef62335f938daa5b84b258a106b96",
          "url": "https://github.com/ivov/lisette/commit/f31d29db48f8cd39ba7ddfc189d2dded7b4f5bd2"
        },
        "date": 1785069900925,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 118965,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30195,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24575,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17027,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13580,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 11011,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6581,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4524,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2764,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1039,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "ec23a60dd2f26f445f3c12bd259ce935d231154b",
          "message": "refactor: decompose complex functions (#1132)",
          "timestamp": "2026-07-26T15:37:58+02:00",
          "tree_id": "a69299a5f660dd6cba1d1de60cf09cce8744b46e",
          "url": "https://github.com/ivov/lisette/commit/ec23a60dd2f26f445f3c12bd259ce935d231154b"
        },
        "date": 1785073099355,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 119200,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30195,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24833,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17082,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13506,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 11027,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6581,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4506,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2764,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1037,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "8114bbcae00744d44e5d8d22a33a12f9aae4d5c4",
          "message": "chore: release v0.10.0 (#1076)",
          "timestamp": "2026-07-26T15:47:26+02:00",
          "tree_id": "958f0941c3409ef88a3c057a651c8069c5adb925",
          "url": "https://github.com/ivov/lisette/commit/8114bbcae00744d44e5d8d22a33a12f9aae4d5c4"
        },
        "date": 1785073668623,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 119200,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30195,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24833,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17082,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13506,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 11027,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6581,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4506,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2764,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1037,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6084,
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
          "id": "0bd658e29df39cc41a73c25e47adc99d6cda477e",
          "message": "feat: SSA nilability analysis for bindgen (#1133)",
          "timestamp": "2026-07-27T23:09:39+02:00",
          "tree_id": "e6ae921ea51280d30ee35096740c6825e187d8f4",
          "url": "https://github.com/ivov/lisette/commit/0bd658e29df39cc41a73c25e47adc99d6cda477e"
        },
        "date": 1785186603634,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 119746,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30195,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24833,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17082,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13506,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 11027,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6581,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4506,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2764,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1037,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6630,
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
          "id": "9d038362530ef7512f3d2dcc89478d2b17ce7688",
          "message": "refactor: derive semantics facts on demand (#1135)",
          "timestamp": "2026-07-27T23:13:11+02:00",
          "tree_id": "bafe0810728ba201ac34c3a9bcafec917edf4b20",
          "url": "https://github.com/ivov/lisette/commit/9d038362530ef7512f3d2dcc89478d2b17ce7688"
        },
        "date": 1785186822480,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 119435,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30186,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24521,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17039,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13569,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 11021,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6572,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4511,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2764,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1037,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6630,
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
          "id": "17fdc6080f1d6add09b49d7ba68853d5983ff084",
          "message": "feat: diagnostic for always-true disjunction (#1137)",
          "timestamp": "2026-07-28T19:20:40+02:00",
          "tree_id": "c0942ab0de06d8e629c73880a5d7b4fdf65056ab",
          "url": "https://github.com/ivov/lisette/commit/17fdc6080f1d6add09b49d7ba68853d5983ff084"
        },
        "date": 1785259268678,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 119620,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30186,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24521,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17216,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13569,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 11021,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6580,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4511,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2764,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1037,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6630,
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
          "id": "2853f1124fcf14467fda0379b090939cd0eeaf58",
          "message": "ci: report fuzz crashes and job failures separately (#1138)",
          "timestamp": "2026-07-28T20:28:00+02:00",
          "tree_id": "7dc51fb10db4a1bbdf1376d269000ee89f279df8",
          "url": "https://github.com/ivov/lisette/commit/2853f1124fcf14467fda0379b090939cd0eeaf58"
        },
        "date": 1785263303343,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 119620,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30186,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24521,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17216,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13569,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 11021,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6580,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4511,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2764,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1037,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6630,
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
          "id": "ae97090c504cd63218f9fe7d30aea5f32782ea3c",
          "message": "feat: diagnostic for constant cast overflow (#1139)",
          "timestamp": "2026-07-28T20:57:02+02:00",
          "tree_id": "519aa0e8985ad8f2eab9d16c4fde3251809d6e2b",
          "url": "https://github.com/ivov/lisette/commit/ae97090c504cd63218f9fe7d30aea5f32782ea3c"
        },
        "date": 1785265049716,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 119729,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30186,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24521,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17311,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13569,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 11021,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6594,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4511,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2764,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1037,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6630,
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
          "id": "0b4229e0c02828db74ca7b7e220d619b61537b55",
          "message": "feat: diagnostic for unconditional recursion (#1140)",
          "timestamp": "2026-07-28T21:13:35+02:00",
          "tree_id": "1ba4a044bbda282293835e952d9bf13067240e79",
          "url": "https://github.com/ivov/lisette/commit/0b4229e0c02828db74ca7b7e220d619b61537b55"
        },
        "date": 1785266044530,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 119939,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30186,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24521,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17513,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13569,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 11021,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6602,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4511,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2764,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1037,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6630,
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
          "id": "8c0f9e126130ed35887dc523be43ee6284a788d7",
          "message": "feat: diagnostic for `fmt.Printf` arity mismatch (#1141)",
          "timestamp": "2026-07-28T21:28:27+02:00",
          "tree_id": "0168e0580561fdb33d147e9f209bcc018f1c90af",
          "url": "https://github.com/ivov/lisette/commit/8c0f9e126130ed35887dc523be43ee6284a788d7"
        },
        "date": 1785266934321,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 120237,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30186,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24521,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17724,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13569,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 11021,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6689,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4511,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2764,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1037,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6630,
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
          "id": "4138d3d28b4b0c1151ad3965f5424d6efb86952f",
          "message": "feat: `lis add --path` for local interop (#1142)",
          "timestamp": "2026-07-28T21:56:35+02:00",
          "tree_id": "7aad3ed3985fb23bf7d0ab5a2a98f936c488e4fe",
          "url": "https://github.com/ivov/lisette/commit/4138d3d28b4b0c1151ad3965f5424d6efb86952f"
        },
        "date": 1785268617063,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 121291,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30186,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24557,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17724,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13569,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 11774,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6720,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4511,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2764,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1271,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6630,
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
          "id": "5fe7420ab14e11340a2bf90d223a315dd2304839",
          "message": "fix: inconsistent escape sequence lexing (#1143)",
          "timestamp": "2026-07-28T23:57:17+02:00",
          "tree_id": "74cda9011c7e664ce0b2e7216438f81314d3c709",
          "url": "https://github.com/ivov/lisette/commit/5fe7420ab14e11340a2bf90d223a315dd2304839"
        },
        "date": 1785275858430,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 121327,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30186,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24524,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17732,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13630,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 11774,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6720,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4511,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2764,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1271,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6630,
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
          "id": "0085e4d6c5959c76412ba2c34b0e3b4c62d4044a",
          "message": "feat: diagnostic for zero-count `strings.Replace` (#1144)",
          "timestamp": "2026-07-29T17:41:31+02:00",
          "tree_id": "5ceab901f6e19f1319b65c518d302ee917c12b88",
          "url": "https://github.com/ivov/lisette/commit/0085e4d6c5959c76412ba2c34b0e3b4c62d4044a"
        },
        "date": 1785339727872,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 121394,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30186,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24524,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17785,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13630,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 11774,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6734,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4511,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2764,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1271,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6630,
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
          "id": "d1fedd04290d74f058dd5f5ff8eca32313b85dd4",
          "message": "feat: diagnostic for duplicate map keys (#1145)",
          "timestamp": "2026-07-29T17:52:06+02:00",
          "tree_id": "1833dc72a249c7436172e655f5d8b6ed88f4bb43",
          "url": "https://github.com/ivov/lisette/commit/d1fedd04290d74f058dd5f5ff8eca32313b85dd4"
        },
        "date": 1785340351746,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "total",
            "value": 121457,
            "unit": "lines"
          },
          {
            "name": "emit",
            "value": 30186,
            "unit": "lines"
          },
          {
            "name": "semantics",
            "value": 24524,
            "unit": "lines"
          },
          {
            "name": "passes",
            "value": 17716,
            "unit": "lines"
          },
          {
            "name": "syntax",
            "value": 13755,
            "unit": "lines"
          },
          {
            "name": "cli",
            "value": 11774,
            "unit": "lines"
          },
          {
            "name": "diagnostics",
            "value": 6741,
            "unit": "lines"
          },
          {
            "name": "lsp",
            "value": 4511,
            "unit": "lines"
          },
          {
            "name": "format",
            "value": 2764,
            "unit": "lines"
          },
          {
            "name": "deps",
            "value": 1271,
            "unit": "lines"
          },
          {
            "name": "stdlib",
            "value": 695,
            "unit": "lines"
          },
          {
            "name": "bindgen",
            "value": 6630,
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
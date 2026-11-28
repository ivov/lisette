//! Splice fuzzer: a mutant that `lis check` accepts must emit without a panic and pass `go build` (`just fuzz-emit`).

#[allow(dead_code, unused_imports)]
mod _harness;

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::mem::{Discriminant, discriminant};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::{self, Command};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

use rayon::prelude::*;
use syntax::ast::Expression;
use syntax::lex::Lexer;
use syntax::parse::Parser;

use _harness::go::emit_program;
use _harness::go::{
    harvest_snapshots, prelude_dir, read_go_toolchain, repo_root, skip_reason_for_imports,
    snapshots_dir,
};
use _harness::lists::read_names;
use _harness::markdown::{backticks_around, code_span};
use _harness::wrap::wrap;
use semantics::AnalysisScope;
use semantics::loader::MemoryLoader;

const SCRIPT_MODULE: &str = "lis-script";
const FAILING: &str = "fuzz_emit/failing.txt";
const WITNESSES: &str = "fuzz_emit/witnesses";

thread_local! {
    static LAST_PANIC: RefCell<String> = const { RefCell::new(String::new()) };
}

static RUNNING: LazyLock<Mutex<HashMap<ThreadId, (Instant, String)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

type Node = (Discriminant<Expression>, usize, usize);

struct Seed {
    source: String,
    nodes: Vec<Node>,
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

fn parse_ok(source: &str) -> Option<Vec<Expression>> {
    let lexed = Lexer::new(source, 0).lex();
    if lexed.failed() {
        return None;
    }
    let parsed = Parser::new(lexed.tokens, source).parse();
    if parsed.has_errors() {
        return None;
    }
    Some(parsed.ast)
}

fn collect_nodes(expression: &Expression, source: &str, nodes: &mut Vec<Node>) {
    let span = expression.get_span();
    let start = span.byte_offset as usize;
    let end = start + span.byte_length as usize;
    if span.byte_length > 0
        && end <= source.len()
        && source.is_char_boundary(start)
        && source.is_char_boundary(end)
    {
        nodes.push((discriminant(expression), start, end));
    }
    for child in expression.children() {
        collect_nodes(child, source, nodes);
    }
}

fn make_seed(source: String) -> Option<Seed> {
    let ast = parse_ok(&source)?;
    let mut nodes = Vec::new();
    for item in &ast {
        collect_nodes(item, &source, &mut nodes);
    }
    if nodes.is_empty() {
        return None;
    }
    Some(Seed { source, nodes })
}

/// Replaces one to three random expressions with expressions of the same AST variant.
fn mutate(
    seeds: &[Seed],
    by_kind: &HashMap<Discriminant<Expression>, Vec<(usize, usize)>>,
    rng: &mut Rng,
) -> String {
    let base = &seeds[rng.below(seeds.len())];
    let mut source = base.source.clone();
    let rounds = 1 + rng.below(3);
    for _ in 0..rounds {
        let Some(seed) = make_seed(source.clone()) else {
            break;
        };
        let (kind, start, end) = seed.nodes[rng.below(seed.nodes.len())];
        let roll = rng.below(10);
        let donor = if roll < 5 {
            // The same program keeps names in scope more often.
            let same: Vec<_> = seed.nodes.iter().filter(|node| node.0 == kind).collect();
            let (_, from, to) = same[rng.below(same.len())];
            seed.source[*from..*to].to_string()
        } else if roll < 9 {
            let pool = &by_kind[&kind];
            let (seed_index, node_index) = pool[rng.below(pool.len())];
            let donor_seed = &seeds[seed_index];
            let (_, from, to) = donor_seed.nodes[node_index];
            donor_seed.source[from..to].to_string()
        } else {
            let (_, from, to) = seed.nodes[rng.below(seed.nodes.len())];
            seed.source[from..to].to_string()
        };
        source.replace_range(start..end, &donor);
    }
    as_script(&source)
}

/// A script needs `fn main`, and a mutation can replace it with another item.
fn as_script(source: &str) -> String {
    if source.lines().any(|line| line.starts_with("fn main()")) {
        return source.to_string();
    }
    let mut calls = String::new();
    if source.lines().any(|line| line.starts_with("fn __test__()")) {
        calls.push_str(" let _ = __test__();");
    }
    if source.lines().any(|line| line.starts_with("fn test()")) {
        calls.push_str(" let _ = test();");
    }
    format!("{source}\nfn main() {{{calls} }}\n")
}

enum Compiled {
    ParseFail,
    Rejected,
    Panic(&'static str, String),
    Emitted(Vec<(String, String)>),
}

/// The front half of `lis build <script>`.
fn compile_script(source: &str) -> Compiled {
    if parse_ok(source).is_none() {
        return Compiled::ParseFail;
    }
    LAST_PANIC.with(|last| last.borrow_mut().clear());
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        let locator = deps::TypedefLocator::default();
        emit_program(
            source,
            "prog.lis",
            SCRIPT_MODULE,
            AnalysisScope::Script {
                inside_project: false,
            },
            &MemoryLoader::new(),
            &locator,
        )
    }));
    match result {
        Ok(result) if !result.errors.is_empty() => Compiled::Rejected,
        Ok(result) if result.output.is_empty() => {
            Compiled::Panic("emit_error", "no output files".to_string())
        }
        Ok(result) => Compiled::Emitted(
            result
                .output
                .iter()
                .map(|file| (file.name.clone(), file.to_go()))
                .collect(),
        ),
        Err(_) => {
            let message = LAST_PANIC.with(|last| last.borrow().clone());
            let kind = if message.contains("crates/emit/") {
                "emit_panic"
            } else {
                "check_panic"
            };
            Compiled::Panic(kind, message)
        }
    }
}

enum Verdict {
    ParseFail,
    Rejected,
    Built,
    Failed {
        signature: String,
        detail: String,
        report: Report,
    },
}

struct Report {
    kind: ReportKind,
    heading: String,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ReportKind {
    Panic,
    GoBuild,
}

impl Report {
    fn panic(message: &str) -> Self {
        let first = message
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("");
        let payload = match first.split_once(": ") {
            Some((location, payload)) if location.contains(".rs:") => payload,
            _ => first,
        };
        let payload = payload
            .trim_start_matches("internal error: ")
            .trim_start_matches("entered unreachable code: ");
        let heading = match payload.split_once(" {") {
            Some((before, _)) => format!("{before} {{ … }}"),
            None => payload.to_string(),
        };
        Self {
            kind: ReportKind::Panic,
            heading,
        }
    }
}

fn hash_of(text: &str) -> String {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// The first line of a failure, with digits removed so that positions do not split one bug.
fn signature(kind: &str, message: &str) -> String {
    let first = message
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    let text: String = first
        .chars()
        .filter(|character| !character.is_ascii_digit())
        .take(160)
        .collect();
    format!("{kind}: {}", text.trim())
}

struct Builder {
    work: PathBuf,
    gocache: PathBuf,
    go_toolchain: String,
    go_time: Duration,
}

impl Builder {
    /// Compiles each program, then runs one `go build` over all emitted packages.
    fn run(&mut self, sources: &[String]) -> Vec<Verdict> {
        let compiled: Vec<Compiled> = sources
            .par_iter()
            .map(|source| {
                let id = thread::current().id();
                RUNNING
                    .lock()
                    .unwrap()
                    .insert(id, (Instant::now(), source.clone()));
                let compiled = compile_script(source);
                RUNNING.lock().unwrap().remove(&id);
                compiled
            })
            .collect();

        let _ = fs::remove_dir_all(&self.work);
        fs::create_dir_all(&self.work).unwrap();
        self.write_go_mod();
        let mut packages = Vec::new();
        for (index, compiled) in compiled.iter().enumerate() {
            if let Compiled::Emitted(files) = compiled {
                let package = format!("p{}", hash_of(&sources[index]));
                let directory = self.work.join(&package);
                fs::create_dir_all(&directory).unwrap();
                for (name, body) in files {
                    fs::write(directory.join(name), body).unwrap();
                }
                packages.push((index, package));
            }
        }
        let errors = if packages.is_empty() {
            HashMap::new()
        } else {
            self.go_build()
        };

        let mut verdicts: Vec<Verdict> = compiled
            .into_iter()
            .map(|compiled| match compiled {
                Compiled::ParseFail => Verdict::ParseFail,
                Compiled::Rejected => Verdict::Rejected,
                Compiled::Panic(kind, message) => Verdict::Failed {
                    signature: signature(kind, &message),
                    report: Report::panic(&message),
                    detail: message,
                },
                Compiled::Emitted(_) => Verdict::Built,
            })
            .collect();
        for (index, package) in packages {
            let Some(output) = errors.get(&package) else {
                continue;
            };
            let first = output
                .lines()
                .find(|line| line.contains(".go:"))
                .unwrap_or(output);
            let parts: Vec<&str> = first.splitn(4, ':').collect();
            let message = parts.get(3).unwrap_or(&first).trim();
            let directory = self.work.join(&package);
            let go = read_go_files(&directory);
            verdicts[index] = Verdict::Failed {
                signature: signature("go_build", message),
                detail: format!("{output}\n---- go ----\n{go}"),
                report: Report {
                    kind: ReportKind::GoBuild,
                    heading: message.to_string(),
                },
            };
        }
        verdicts
    }

    fn write_go_mod(&self) {
        let prelude = prelude_dir().canonicalize().unwrap();
        let content = format!(
            "module {SCRIPT_MODULE}\n\ngo {}\n\nrequire github.com/ivov/lisette/prelude v0.0.0\n\nreplace github.com/ivov/lisette/prelude => {}\n",
            self.go_toolchain,
            prelude.display()
        );
        fs::write(self.work.join("go.mod"), content).unwrap();
    }

    /// The `go build` errors of each failing package, with one retry after a cache race.
    fn go_build(&mut self) -> HashMap<String, String> {
        let started = Instant::now();
        let mut text = self.go_build_once();
        if text.contains("could not import") || text.contains("no such file or directory") {
            eprintln!("go build hit a cache error, building the batch again");
            text = self.go_build_once();
        }
        self.go_time += started.elapsed();
        let mut errors: HashMap<String, String> = HashMap::new();
        let mut current: Option<String> = None;
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("# ") {
                current = rest.rsplit('/').next().map(str::to_string);
                continue;
            }
            let package = current.clone().or_else(|| {
                line.split('/')
                    .find(|part| part.starts_with('p') && part.len() == 17)
                    .map(str::to_string)
            });
            if let Some(package) = package {
                errors
                    .entry(package)
                    .or_default()
                    .push_str(&format!("{line}\n"));
            }
        }
        errors
    }

    fn go_build_once(&self) -> String {
        let output = Command::new("go")
            .args(["build", "./..."])
            .current_dir(&self.work)
            .env("GOCACHE", &self.gocache)
            .env("NO_COLOR", "1")
            .output()
            .expect("spawn go build");
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    }
}

fn read_go_files(directory: &Path) -> String {
    let mut paths: Vec<_> = fs::read_dir(directory)
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default();
    paths.sort();
    paths
        .iter()
        .map(|path| fs::read_to_string(path).unwrap_or_default())
        .collect()
}

struct KnownSignatures {
    by_signature: BTreeMap<String, String>,
}

impl KnownSignatures {
    fn read() -> Self {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(FAILING);
        let content = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        let mut by_signature = BTreeMap::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (witness, signature) = line
                .split_once(' ')
                .unwrap_or_else(|| panic!("{FAILING}: no signature in `{line}`"));
            by_signature.insert(signature.trim().to_string(), witness.to_string());
        }
        Self { by_signature }
    }

    fn witness_source(witness: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(WITNESSES)
            .join(format!("{witness}.lis"));
        fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read witness {}: {error}", path.display()))
    }
}

/// A file name for a new signature: its words, then a hash so that two names never clash.
fn witness_name(signature: &str) -> String {
    let words: String = signature
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let words: Vec<&str> = words.split('-').filter(|word| !word.is_empty()).collect();
    let prefix = words.iter().take(8).copied().collect::<Vec<_>>().join("-");
    format!("{prefix}-{}", &hash_of(signature)[..6])
}

struct Findings {
    out: PathBuf,
    known: KnownSignatures,
    hits: BTreeMap<String, u64>,
    new: Vec<(Report, String)>,
}

impl Findings {
    fn record(&mut self, source: &str, signature: &str, detail: &str, report: Report) {
        let count = self.hits.entry(signature.to_string()).or_insert(0);
        *count += 1;
        if *count > 1 || self.known.by_signature.contains_key(signature) {
            return;
        }
        let directory = self.out.join("new");
        fs::create_dir_all(&directory).unwrap();
        let name = witness_name(signature);
        fs::write(directory.join(format!("{name}.lis")), source).unwrap();
        fs::write(directory.join(format!("{name}.txt")), detail).unwrap();
        eprintln!("NEW {name} {signature}");
        self.new.push((report, source.to_string()));
    }

    fn new_signatures(&self) -> Vec<&String> {
        self.hits
            .keys()
            .filter(|signature| !self.known.by_signature.contains_key(*signature))
            .collect()
    }
}

fn read_seeds() -> Vec<Seed> {
    let typecheck_only = read_names("go_emit/typecheck_only.txt");
    let mut sources: Vec<String> = harvest_snapshots(&snapshots_dir("emit"))
        .into_iter()
        .filter(|test| {
            !typecheck_only.contains(&test.name)
                && skip_reason_for_imports(&test.input, &test.snap_body).is_none()
        })
        .map(|test| as_script(&wrap(&test.input)))
        .collect();
    let corpus = repo_root().join("fuzz/seed_corpus");
    let mut paths: Vec<_> = fs::read_dir(&corpus)
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default();
    paths.sort();
    for path in paths {
        if let Ok(source) = fs::read_to_string(&path) {
            sources.push(as_script(&wrap(&source)));
        }
    }
    // Go receives `@rawgo` text as written, so its mutants test nothing in emit.
    sources
        .into_iter()
        .filter(|source| !source.contains("@rawgo"))
        .filter_map(make_seed)
        .collect()
}

fn environment_number(name: &str, default: u64) -> u64 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn install_panic_hook() {
    panic::set_hook(Box::new(|info| {
        let location = info
            .location()
            .map(|location| format!("{}:{}", location.file(), location.line()))
            .unwrap_or_default();
        let payload = info
            .payload()
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| {
                info.payload()
                    .downcast_ref::<&str>()
                    .map(|text| text.to_string())
            })
            .unwrap_or_default();
        LAST_PANIC.with(|last| *last.borrow_mut() = format!("{location}: {payload}"));
    }));
}

/// Saves a program that runs over 30 s in check or emit, then ends the process.
fn start_hang_watch(out: PathBuf) {
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_secs(2));
            let stuck = RUNNING
                .lock()
                .unwrap()
                .values()
                .find(|(started, _)| started.elapsed() > Duration::from_secs(30))
                .map(|(_, source)| source.clone());
            if let Some(source) = stuck {
                let directory = out.join("hang");
                fs::create_dir_all(&directory).unwrap();
                fs::write(directory.join(format!("{}.lis", hash_of(&source))), &source).unwrap();
                eprintln!(
                    "HANG: a program ran over 30 s in check or emit, saved in {}",
                    directory.display()
                );
                process::exit(3);
            }
        }
    });
}

fn replay_witnesses(
    builder: &mut Builder,
    known: &KnownSignatures,
) -> Vec<(String, Option<String>)> {
    let entries: Vec<(&String, &String)> = known.by_signature.iter().collect();
    let sources: Vec<String> = entries
        .iter()
        .map(|(_, witness)| KnownSignatures::witness_source(witness))
        .collect();
    let verdicts = builder.run(&sources);
    let mut problems = Vec::new();
    for ((expected, witness), verdict) in entries.into_iter().zip(verdicts) {
        match verdict {
            Verdict::Failed { signature, .. } if &signature == expected => {}
            Verdict::Failed { signature, .. } => problems.push((witness.clone(), Some(signature))),
            Verdict::ParseFail | Verdict::Rejected | Verdict::Built => {
                problems.push((witness.clone(), None))
            }
        }
    }
    problems
}

const ISSUE_LIMIT: usize = 40_000;

fn witness_status(signature: Option<&str>) -> String {
    match signature {
        Some(signature) => format!("now fails with {signature}"),
        None => "passes".to_string(),
    }
}

fn issue_body(new: &[(Report, String)], witness_problems: &[(String, Option<String>)]) -> String {
    let mut entries: Vec<&(Report, String)> = new.iter().collect();
    entries.sort_by(|a, b| (a.0.kind, &a.0.heading).cmp(&(b.0.kind, &b.0.heading)));
    entries.dedup_by(|a, b| a.0.kind == b.0.kind && a.0.heading == b.0.heading);

    let mut body = String::new();
    let mut shown = 0;
    let mut omitted = 0;
    for (report, source) in &entries {
        let title = match report.kind {
            ReportKind::Panic => "Panic",
            ReportKind::GoBuild => "Build error",
        };
        let source = source.trim_matches('\n');
        let fence = backticks_around(source, 3);
        let entry = format!(
            "\n**{}. {title}: {}**\n\n{fence}rust\n{source}\n{fence}\n",
            shown + 1,
            code_span(&report.heading),
        );
        if body.len() + entry.len() > ISSUE_LIMIT {
            omitted += 1;
            continue;
        }
        shown += 1;
        body.push_str(&entry);
    }
    if !witness_problems.is_empty() {
        body.push_str(&format!(
            "\n**{}. Stale failures**\n\nUpdate their lines in `tests/{FAILING}`, and delete the saved programs that pass from `tests/{WITNESSES}`.\n\n",
            shown + 1
        ));
        for (witness, signature) in witness_problems {
            body.push_str(&format!(
                "- {}: {}\n",
                code_span(witness),
                witness_status(signature.as_deref().map(code_span).as_deref())
            ));
        }
    }
    if omitted > 0 {
        body.push_str(&format!(
            "\n{omitted} more errors do not fit in this issue. They are in the `emitter-fuzzing-findings` artifact.\n"
        ));
    }
    body.trim_start().to_string()
}

#[test]
fn random_programs_give_valid_go() {
    if env::var("FUZZ_SECONDS").is_err() {
        eprintln!("skipping random_programs_give_valid_go: set FUZZ_SECONDS to run it");
        return;
    }
    let seconds = environment_number("FUZZ_SECONDS", 60);
    let seed_value = environment_number("FUZZ_SEED", 0x9e37_79b9_7f4a_7c15);
    let batch = environment_number("FUZZ_BATCH", 256) as usize;
    let fuzz_root = repo_root().join("target/fuzz_emit");
    let out = env::var("FUZZ_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| fuzz_root.join("out"));
    let _ = fs::remove_dir_all(out.join("new"));
    let _ = fs::remove_file(out.join("issue.md"));
    fs::create_dir_all(&out).unwrap();

    install_panic_hook();
    start_hang_watch(out.clone());

    let mut builder = Builder {
        work: fuzz_root.join("work"),
        gocache: fuzz_root.join("gocache"),
        go_toolchain: read_go_toolchain().unwrap(),
        go_time: Duration::ZERO,
    };
    let known = KnownSignatures::read();
    let witness_problems = replay_witnesses(&mut builder, &known);
    for (witness, signature) in &witness_problems {
        eprintln!(
            "WITNESS {witness}: {}",
            witness_status(signature.as_deref())
        );
    }

    let seeds = read_seeds();
    let mut by_kind: HashMap<Discriminant<Expression>, Vec<(usize, usize)>> = HashMap::new();
    for (seed_index, seed) in seeds.iter().enumerate() {
        for (node_index, node) in seed.nodes.iter().enumerate() {
            by_kind
                .entry(node.0)
                .or_default()
                .push((seed_index, node_index));
        }
    }
    eprintln!("seeds: {}", seeds.len());

    let mut findings = Findings {
        out: out.clone(),
        known,
        hits: BTreeMap::new(),
        new: Vec::new(),
    };
    let counts: [AtomicU64; 5] = Default::default();
    let [generated, parse_fail, rejected, built, failed] = &counts;
    let mut seen: HashSet<String> = HashSet::new();
    let started = Instant::now();
    let mut round: u64 = 0;

    while started.elapsed() < Duration::from_secs(seconds) {
        round += 1;
        let sources: Vec<String> = (0..batch * 8)
            .into_par_iter()
            .map(|index| {
                let mut rng = Rng(seed_value
                    ^ (round << 32)
                    ^ (index as u64).wrapping_mul(0x2545_f491_4f6c_dd1d)
                    | 1);
                for _ in 0..4 {
                    rng.next();
                }
                mutate(&seeds, &by_kind, &mut rng)
            })
            .collect();
        let sources: Vec<String> = sources
            .into_iter()
            .filter(|source| seen.insert(hash_of(source)))
            .collect();
        generated.fetch_add(sources.len() as u64, Ordering::Relaxed);
        let verdicts = builder.run(&sources);
        for (source, verdict) in sources.iter().zip(verdicts) {
            match verdict {
                Verdict::ParseFail => parse_fail.fetch_add(1, Ordering::Relaxed),
                Verdict::Rejected => rejected.fetch_add(1, Ordering::Relaxed),
                Verdict::Built => built.fetch_add(1, Ordering::Relaxed),
                Verdict::Failed {
                    signature,
                    detail,
                    report,
                } => {
                    findings.record(source, &signature, &detail, report);
                    failed.fetch_add(1, Ordering::Relaxed)
                }
            };
        }
        eprintln!(
            "[{:>5.0}s] generated {} parse_fail {} rejected {} built {} failed {} signatures {}",
            started.elapsed().as_secs_f64(),
            generated.load(Ordering::Relaxed),
            parse_fail.load(Ordering::Relaxed),
            rejected.load(Ordering::Relaxed),
            built.load(Ordering::Relaxed),
            failed.load(Ordering::Relaxed),
            findings.hits.len(),
        );
    }

    let mut summary = format!(
        "elapsed {:.1}s, go build {:.1}s\ngenerated {}\nparse_fail {}\nrejected {}\nbuilt {}\nfailed {}\n\nsignatures (hits, witness or NEW):\n",
        started.elapsed().as_secs_f64(),
        builder.go_time.as_secs_f64(),
        generated.load(Ordering::Relaxed),
        parse_fail.load(Ordering::Relaxed),
        rejected.load(Ordering::Relaxed),
        built.load(Ordering::Relaxed),
        failed.load(Ordering::Relaxed),
    );
    let mut rows: Vec<_> = findings.hits.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (signature, count) in rows {
        let witness = findings
            .known
            .by_signature
            .get(signature)
            .cloned()
            .unwrap_or_else(|| "NEW".to_string());
        summary.push_str(&format!("{count:>7}  {witness}  {signature}\n"));
    }
    let new_signatures = findings.new_signatures();
    if !new_signatures.is_empty() {
        summary.push_str(&format!(
            "\nLines to add to tests/{FAILING} after review (programs in {}):\n",
            out.join("new").display()
        ));
        for signature in &new_signatures {
            summary.push_str(&format!("{} {signature}\n", witness_name(signature)));
        }
    }
    fs::write(out.join("summary.txt"), &summary).unwrap();
    if !findings.new.is_empty() || !witness_problems.is_empty() {
        fs::write(
            out.join("issue.md"),
            issue_body(&findings.new, &witness_problems),
        )
        .unwrap();
    }
    eprintln!("{summary}");

    assert!(
        witness_problems.is_empty() && new_signatures.is_empty(),
        "{} witness(es) no longer match tests/{FAILING}, and {} new failure signature(s) were found (each one is printed above)",
        witness_problems.len(),
        new_signatures.len(),
    );
}

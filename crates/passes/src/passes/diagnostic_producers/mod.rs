mod generics;
mod unused_expressions;

use diagnostics::LisetteDiagnostic;
use rayon::prelude::*;

use semantics::store::Store;

use super::{PARALLEL_THRESHOLD, source_file_work};

pub(crate) fn run_all(store: &Store) -> Vec<LisetteDiagnostic> {
    let work = source_file_work(store);

    if work.len() < PARALLEL_THRESHOLD {
        let mut local = Vec::new();
        for (package, file) in &work {
            generics::run(&file.items, &mut local);
            unused_expressions::run(&file.items, &package.id, store, &mut local);
        }
        return local;
    }

    let locals: Vec<Vec<LisetteDiagnostic>> = work
        .par_iter()
        .map(|(package, file)| {
            let mut local = Vec::new();
            generics::run(&file.items, &mut local);
            unused_expressions::run(&file.items, &package.id, store, &mut local);
            local
        })
        .collect();

    locals.into_iter().flatten().collect()
}

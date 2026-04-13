pub(crate) fn executor_name(base: &str) -> String {
    format!("{base}-side-effect-executor")
}

pub(crate) fn loader_name(base: &str) -> String {
    format!("{base}-side-effect-loader")
}

pub(crate) fn runner_name(base: &str) -> String {
    format!("{base}-side-effect-runner")
}

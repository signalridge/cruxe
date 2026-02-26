pub fn rust_entry() {
    rust_process();
}

pub fn rust_process() {
    rust_validate();
    rust_recurse(1);
    rust_external::call();
    let worker = RustWorker {};
    worker.rust_method();
}

pub fn rust_validate() {}

pub fn rust_recurse(n: i32) {
    if n > 0 {
        rust_recurse(n - 1);
    }
}

pub struct RustWorker;

impl RustWorker {
    pub fn rust_method(&self) {
        rust_validate();
        rust_helper();
    }
}

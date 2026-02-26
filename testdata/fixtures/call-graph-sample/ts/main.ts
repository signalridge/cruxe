import { tsHelper } from "./util";

class TsWorker {
  run() {
    tsValidate();
  }
}

export function tsEntry() {
  tsProcess();
}

export function tsProcess() {
  tsValidate();
  tsHelper();
  tsRecurse(1);
  const worker = new TsWorker();
  worker.run();
  Math.max(1, 2);
}

export function tsValidate() {}

export function tsRecurse(n: number) {
  if (n > 0) {
    tsRecurse(n - 1);
  }
}

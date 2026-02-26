package main

func goEntry() {
	goProcess()
}

func goProcess() {
	goValidate()
	goHelper()
	goRecurse(1)
	goExternal()
	var worker GoWorker
	worker.Run()
}

func goValidate() {}

func goRecurse(n int) {
	if n > 0 {
		goRecurse(n - 1)
	}
}

type GoWorker struct{}

func (w GoWorker) Run() {
	goValidate()
}

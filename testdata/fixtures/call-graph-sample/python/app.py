from helpers import py_helper


class PyWorker:
    def run(self):
        py_validate()


def py_entry():
    py_process()


def py_process():
    py_validate()
    py_helper()
    py_recurse(1)
    worker = PyWorker()
    worker.run()
    print("x")


def py_validate():
    return True


def py_recurse(n):
    if n > 0:
        py_recurse(n - 1)

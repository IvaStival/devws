.PHONY: install install-deps uninstall check

install:
	@./install.sh

install-deps:
	@./install.sh --deps

uninstall:
	@./delete.sh

check:
	@./scripts/check.sh

.PHONY: install install-deps install-iterm-keys uninstall check

install:
	@./install.sh

install-deps:
	@./install.sh --deps

install-iterm-keys:
	@./scripts/setup_iterm_keys.sh

uninstall:
	@./delete.sh

check:
	@./scripts/check.sh

TARGET?=x86_64-unknown-uefi-drv

export LD=ld
export RUST_TARGET_PATH=$(CURDIR)/targets
BUILD=build/$(TARGET)

all: $(BUILD)/boot.efi

clean:
	cargo clean
	rm -rf build

update:
	git submodule update --init --recursive --remote
	cargo update

$(BUILD)/OVMF_VARS.fd: /usr/share/OVMF/OVMF_VARS.fd
	cp $< $@

qemu: $(BUILD)/boot.img $(BUILD)/OVMF_VARS.fd
	kvm -M q35 -m 1024 -net none -vga std $< \
		-drive if=pflash,format=raw,readonly,file=/usr/share/OVMF/OVMF_CODE.fd \
		-drive if=pflash,format=raw,file=$(BUILD)/OVMF_VARS.fd \
		-chardev stdio,id=debug -device isa-debugcon,iobase=0x402,chardev=debug

$(BUILD)/boot.img: $(BUILD)/efi.img
	dd if=/dev/zero of=$@.tmp bs=512 count=100352
	parted $@.tmp -s -a minimal mklabel gpt
	parted $@.tmp -s -a minimal mkpart EFI FAT16 2048s 93716s
	parted $@.tmp -s -a minimal toggle 1 boot
	dd if=$< of=$@.tmp bs=512 count=98304 seek=2048 conv=notrunc
	mv $@.tmp $@

$(BUILD)/efi.img: $(BUILD)/boot.efi
	dd if=/dev/zero of=$@.tmp bs=512 count=98304
	mkfs.vfat $@.tmp
	mmd -i $@.tmp efi
	mmd -i $@.tmp efi/boot
	mcopy -i $@.tmp $< ::driver.efi
	mcopy -i $@.tmp res/startup.nsh ::startup.nsh
	mv $@.tmp $@

$(BUILD)/boot.efi: Cargo.lock Cargo.toml res/* src/* src/*/*
	mkdir -p $(BUILD)
	cargo rustc \
		-Z build-std=core,alloc \
		--target $(TARGET) \
		--release \
		-- \
		-C soft-float \
		--emit link=$@

BINUTILS=2.28.1

prefix/binutils-$(BINUTILS).tar.xz:
	mkdir -p "`dirname $@`"
	wget "https://ftp.gnu.org/gnu/binutils/binutils-$(BINUTILS).tar.xz" -O "$@.partial"
	sha384sum -c binutils.sha384
	mv "$@.partial" "$@"

prefix/binutils-$(BINUTILS): prefix/binutils-$(BINUTILS).tar.xz
	mkdir -p "$@.partial"
	tar --extract --verbose --file "$<" --directory "$@.partial" --strip-components=1
	mv "$@.partial" "$@"

$(LD): prefix/binutils-$(BINUTILS)
	rm -rf prefix/bin prefix/share "prefix/$(TARGET)"
	mkdir -p prefix/build
	cd prefix/build && \
	../../$</configure --target="$(TARGET)" --disable-werror --prefix="$(PREFIX)" && \
	make all-ld -j `nproc` && \
	make install-ld -j `nproc`

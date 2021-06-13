ifeq ($(OS),Windows_NT)

all: aes-win files-win

aes-win: src/network/aes/aes.c src/network/aes/gmult.c
	gcc -fPIC -shared -o src/network/aes/aes.dll src/network/aes/aes.c src/network/aes/gmult.c

files-win: src/hardware/clipboard/_win32/file2clip.cs
	cd src/hardware/clipboard/_win32 && csc file2clip.cs

else

UNAME_S := $(shell uname -s)

ifeq ($(UNAME_S),Linux)
all: mod-lin aes-unix 
mod-lin:
	chmod +x src/hardware/clipboard/_xorg/xcopy.sh; \
	chmod +x src/hardware/clipboard/_xorg/xpaste.sh
endif
ifeq ($(UNAME_S),Darwin)
all: mod-mac aes-unix
mod-mac:
	chmod +x src/hardware/clipboard/_darwin/mcopy.sh; \
	chmod +x src/hardware/clipboard/_darwin/mpaste.sh; \
	chmod +x src/hardware/clipboard/_darwin/file2clip.applescript; \
	chmod +x src/hardware/clipboard/_darwin/getfiles.applescript
endif

aes-unix: src/network/aes/aes.c src/network/aes/gmult.c
	gcc -fPIC -shared -o src/network/aes/aes.so src/network/aes/aes.c src/network/aes/gmult.c

endif

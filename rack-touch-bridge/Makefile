RACK_DIR ?= ../..

SOURCES += $(wildcard src/*.cpp)

DISTRIBUTABLES += res
DISTRIBUTABLES += $(wildcard LICENSE*) plugin.json

ifdef ARCH_WIN
LDFLAGS += -lws2_32
endif

include $(RACK_DIR)/plugin.mk

#include "plugin.hpp"

Plugin* pluginInstance = nullptr;

void init(Plugin* p) {
    pluginInstance = p;
    p->addModel(modelBridge);
}

#include "plugin.hpp"
#include "server.hpp"

#include <jansson.h>

struct BridgeModule : Module {
    BridgeModule() {
        config(0, 0, 0, 0);
    }
};

static json_t* rect4(double x, double y, double w, double h) {
    json_t* a = json_array();
    json_array_append_new(a, json_real(x));
    json_array_append_new(a, json_real(y));
    json_array_append_new(a, json_real(w));
    json_array_append_new(a, json_real(h));
    return a;
}

static json_t* portJson(PortWidget* port, bool isInput) {
    json_t* o = json_object();
    json_object_set_new(o, "id", json_integer(port->portId));
    Vec center = port->getAbsoluteOffset(port->box.size.div(2.0));
    json_object_set_new(o, "x", json_real(center.x));
    json_object_set_new(o, "y", json_real(center.y));
    if (port->module) {
        engine::PortInfo* pi = isInput
            ? port->module->getInputInfo(port->portId)
            : port->module->getOutputInfo(port->portId);
        if (pi) json_object_set_new(o, "name", json_string(pi->name.c_str()));
    }
    return o;
}

static json_t* paramJson(ParamWidget* pw) {
    json_t* o = json_object();
    json_object_set_new(o, "id", json_integer(pw->paramId));
    Vec center = pw->getAbsoluteOffset(pw->box.size.div(2.0));
    json_object_set_new(o, "x", json_real(center.x));
    json_object_set_new(o, "y", json_real(center.y));
    engine::ParamQuantity* pq = pw->getParamQuantity();
    if (pq) json_object_set_new(o, "name", json_string(pq->name.c_str()));
    return o;
}

static json_t* moduleJson(ModuleWidget* mw) {
    json_t* o = json_object();
    json_object_set_new(o, "id", json_integer(mw->module ? mw->module->id : -1));
    json_object_set_new(o, "name",
        json_string(mw->model ? mw->model->name.c_str() : ""));

    Vec tl = mw->getAbsoluteOffset(Vec(0, 0));
    json_object_set_new(o, "screenBox",
        rect4(tl.x, tl.y, mw->box.size.x, mw->box.size.y));

    json_t* params = json_array();
    for (ParamWidget* p : mw->getParams())
        json_array_append_new(params, paramJson(p));
    json_object_set_new(o, "params", params);

    json_t* inputs = json_array();
    for (PortWidget* p : mw->getInputs())
        json_array_append_new(inputs, portJson(p, true));
    json_object_set_new(o, "inputs", inputs);

    json_t* outputs = json_array();
    for (PortWidget* p : mw->getOutputs())
        json_array_append_new(outputs, portJson(p, false));
    json_object_set_new(o, "outputs", outputs);

    return o;
}

static json_t* hoveredJson() {
    json_t* o = json_object();
    json_object_set_new(o, "kind", json_string("none"));

    Widget* w = APP->event ? APP->event->hoveredWidget : nullptr;
    while (w) {
        if (auto* pw = dynamic_cast<ParamWidget*>(w)) {
            json_object_set_new(o, "kind", json_string("param"));
            json_object_set_new(o, "paramId", json_integer(pw->paramId));
            if (pw->module)
                json_object_set_new(o, "moduleId", json_integer(pw->module->id));
            return o;
        }
        if (auto* port = dynamic_cast<PortWidget*>(w)) {
            const char* kind =
                port->type == engine::Port::INPUT ? "input" : "output";
            json_object_set_new(o, "kind", json_string(kind));
            json_object_set_new(o, "portId", json_integer(port->portId));
            if (port->module)
                json_object_set_new(o, "moduleId", json_integer(port->module->id));
            return o;
        }
        if (auto* mwh = dynamic_cast<ModuleWidget*>(w)) {
            json_object_set_new(o, "kind", json_string("module"));
            if (mwh->module)
                json_object_set_new(o, "moduleId", json_integer(mwh->module->id));
            return o;
        }
        w = w->parent;
    }
    return o;
}

struct BridgeWidget : ModuleWidget {
    BridgeServer server;
    int frameCounter = 0;

    BridgeWidget(BridgeModule* module) {
        setModule(module);
        setPanel(createPanel(asset::plugin(pluginInstance, "res/Bridge.svg")));
        if (module) server.start(54321);
    }

    ~BridgeWidget() override {
        server.stop();
    }

    void step() override {
        ModuleWidget::step();
        if (!module) return;
        if (++frameCounter < 6) return;
        frameCounter = 0;

        json_t* root = json_object();
        json_object_set_new(root, "t", json_real(system::getTime()));

        Vec ws = APP->window->getSize();
        json_t* win = json_object();
        json_object_set_new(win, "w", json_real(ws.x));
        json_object_set_new(win, "h", json_real(ws.y));
        json_object_set_new(root, "window", win);

        json_t* view = json_object();
        json_object_set_new(view, "zoom",
            json_real(APP->scene->rackScroll->getZoom()));
        json_object_set_new(root, "view", view);

        json_object_set_new(root, "hovered", hoveredJson());

        json_t* mods = json_array();
        for (ModuleWidget* mw : APP->scene->rack->getModules())
            json_array_append_new(mods, moduleJson(mw));
        json_object_set_new(root, "modules", mods);

        char* s = json_dumps(root, JSON_COMPACT);
        json_decref(root);
        if (s) {
            server.publish(std::string(s));
            std::free(s);
        }
    }
};

Model* modelBridge = createModel<BridgeModule, BridgeWidget>("Bridge");

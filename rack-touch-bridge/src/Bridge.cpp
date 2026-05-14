#include "plugin.hpp"
#include "server.hpp"

#include <jansson.h>

#include <cstdint>
#include <set>
#include <string>
#include <vector>

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
    if (mw->model) {
        json_object_set_new(o, "modelSlug",
            json_string(mw->model->slug.c_str()));
        if (mw->model->plugin)
            json_object_set_new(o, "pluginSlug",
                json_string(mw->model->plugin->slug.c_str()));
    }
    if (mw->module)
        json_object_set_new(o, "bypassed",
            json_boolean(mw->module->isBypassed()));

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

// Look up a ModuleWidget by Rack module ID. Avoids assuming any specific
// RackWidget lookup API across SDK point releases.
static ModuleWidget* findModuleWidget(std::int64_t id) {
    for (ModuleWidget* mw : APP->scene->rack->getModules())
        if (mw->module && mw->module->id == id) return mw;
    return nullptr;
}

// Try to find the on-disk path of a module's panel SVG. Walks the widget
// tree for an app::SvgPanel first (works for any plugin that uses the
// stock panel widget) and falls back to the conventional res/<slug>.svg
// location relative to the plugin's directory.
static std::string findPanelSvgPath(ModuleWidget* mw) {
    std::vector<widget::Widget*> stack(mw->children.begin(), mw->children.end());
    while (!stack.empty()) {
        widget::Widget* w = stack.back();
        stack.pop_back();
        if (auto* sp = dynamic_cast<app::SvgPanel*>(w)) {
            if (sp->sw && sp->sw->svg) return sp->sw->svg->path;
        }
        for (widget::Widget* c : w->children) stack.push_back(c);
    }
    if (mw->model && mw->model->plugin)
        return mw->model->plugin->path + "/res/" + mw->model->slug + ".svg";
    return {};
}

static std::string readFileQuietly(const std::string& path) {
    if (path.empty()) return {};
    try { return system::readFile(path); }
    catch (...) { return {}; }
}

static void sendJsonFrame(BridgeServer& server, json_t* root) {
    char* s = json_dumps(root, JSON_COMPACT);
    if (s) {
        server.send_one(std::string(s));
        std::free(s);
    }
}

struct BridgeWidget : ModuleWidget {
    BridgeServer server;
    int frameCounter = 0;
    std::set<std::string> sentAssets;

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
        drainInbound();
        if (++frameCounter < 6) return;
        frameCounter = 0;
        emitAssetFrames();
        emitSnapshot();
    }

    void drainInbound() {
        auto lines = server.drain_inbound();
        for (const std::string& line : lines) {
            json_error_t err;
            json_t* msg = json_loads(line.c_str(), 0, &err);
            if (!msg) continue;
            handleCommand(msg);
            json_decref(msg);
        }
    }

    void handleCommand(json_t* msg) {
        const char* op = json_string_value(json_object_get(msg, "op"));
        if (!op) return;
        std::string opS = op;
        if (opS == "module-action") {
            std::int64_t seq =
                (std::int64_t)json_integer_value(json_object_get(msg, "seq"));
            std::int64_t moduleId =
                (std::int64_t)json_integer_value(json_object_get(msg, "moduleId"));
            const char* action =
                json_string_value(json_object_get(msg, "action"));
            const char* errReason =
                applyAction(moduleId, action ? action : "");
            sendActionAck(seq, errReason);
        }
    }

    const char* applyAction(std::int64_t moduleId, const std::string& action) {
        ModuleWidget* target = findModuleWidget(moduleId);
        if (!target) return "module-not-found";

        if (action == "bypass") {
            if (!target->module) return "no-engine-module";
            APP->engine->bypassModule(target->module,
                                      !target->module->isBypassed());
            return nullptr;
        }
        if (action == "disconnect") { target->disconnectAction(); return nullptr; }
        if (action == "reset")      { target->resetAction();      return nullptr; }
        if (action == "randomize")  { target->randomizeAction();  return nullptr; }
        if (action == "clone")      { target->cloneAction();      return nullptr; }
        if (action == "delete")     { target->removeAction();     return nullptr; }
        return "unknown-action";
    }

    void sendActionAck(std::int64_t seq, const char* errReason) {
        json_t* r = json_object();
        json_object_set_new(r, "op", json_string("action-ack"));
        json_object_set_new(r, "seq", json_integer(seq));
        json_object_set_new(r, "ok", errReason ? json_false() : json_true());
        if (errReason)
            json_object_set_new(r, "reason", json_string(errReason));
        sendJsonFrame(server, r);
        json_decref(r);
    }

    void emitAssetFrames() {
        for (ModuleWidget* mw : APP->scene->rack->getModules()) {
            if (!mw->model || !mw->model->plugin) continue;
            std::string key = mw->model->plugin->slug + "/" + mw->model->slug;
            if (sentAssets.count(key)) continue;
            sentAssets.insert(key);

            std::string svg = readFileQuietly(findPanelSvgPath(mw));
            if (svg.empty()) continue;

            json_t* a = json_object();
            json_object_set_new(a, "op",         json_string("module-asset"));
            json_object_set_new(a, "v",          json_integer(1));
            json_object_set_new(a, "pluginSlug", json_string(mw->model->plugin->slug.c_str()));
            json_object_set_new(a, "modelSlug",  json_string(mw->model->slug.c_str()));
            json_object_set_new(a, "format",     json_string("svg"));
            json_object_set_new(a, "data",       json_string(svg.c_str()));
            sendJsonFrame(server, a);
            json_decref(a);
        }
    }

    void emitSnapshot() {
        json_t* root = json_object();
        json_object_set_new(root, "op", json_string("snapshot"));
        json_object_set_new(root, "v",  json_integer(1));
        json_object_set_new(root, "t",  json_real(system::getTime()));

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

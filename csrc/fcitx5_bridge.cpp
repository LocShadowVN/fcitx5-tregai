// SPDX-License-Identifier: GPL-3.0-or-later

#include <chrono>
#include <cstdint>
#include <cstring>
#include <fcitx-utils/capabilityflags.h>
#include <fcitx-utils/key.h>
#include <fcitx-utils/keysym.h>
#include <fcitx/addonfactory.h>
#include <fcitx/addoninstance.h>
#include <fcitx/addonmanager.h>
#include <fcitx/inputcontext.h>
#include <fcitx/inputcontextproperty.h>
#include <fcitx/inputmethodengine.h>
#include <fcitx/inputpanel.h>
#include <fcitx/instance.h>
#include <fcitx/text.h>
#include <fcitx/userinterface.h>
#include <string>
#include <thread>

extern "C" {
struct VietImeContext;

struct FfiKeyResult {
    bool handled;
    char *commit_text;
    char *preedit_text;
    uint32_t backspace_count;
    bool underline_preedit;
    bool direct_mode;
    uint32_t commit_delay_ms;
};

VietImeContext *viet_ime_context_new();
void viet_ime_context_free(VietImeContext *ctx);
void viet_ime_context_reset(VietImeContext *ctx);
void viet_ime_context_set_program(VietImeContext *ctx, const char *program);
void viet_ime_context_reload_config(VietImeContext *ctx);
FfiKeyResult viet_ime_context_flush(VietImeContext *ctx);
FfiKeyResult viet_ime_context_process_key(VietImeContext *ctx,
                                          uint32_t keysym,
                                          uint32_t state,
                                          bool is_release);
void viet_ime_free_result(FfiKeyResult res);
}

namespace {

class TreGaiState final : public fcitx::InputContextProperty {
public:
    TreGaiState() : ctx_(viet_ime_context_new()) {}
    ~TreGaiState() override {
        if (ctx_) {
            viet_ime_context_free(ctx_);
            ctx_ = nullptr;
        }
    }

    TreGaiState(const TreGaiState &) = delete;
    TreGaiState &operator=(const TreGaiState &) = delete;

    VietImeContext *ctx() const { return ctx_; }

private:
    VietImeContext *ctx_;
};

class TreGaiEngine final : public fcitx::InputMethodEngineV2 {
public:
    explicit TreGaiEngine(fcitx::Instance *instance)
        : instance_(instance),
          factory_([](fcitx::InputContext &) { return new TreGaiState(); }) {
        instance_->inputContextManager().registerProperty("treGaiState", &factory_);
    }

    void activate(const fcitx::InputMethodEntry &,
                  fcitx::InputContextEvent &event) override {
        auto *ic = event.inputContext();
        auto *state = ic->propertyFor(&factory_);
        viet_ime_context_set_program(state->ctx(), ic->program().c_str());
        viet_ime_context_reset(state->ctx());
    }

    void deactivate(const fcitx::InputMethodEntry &entry,
                    fcitx::InputContextEvent &event) override {
        auto *ic = event.inputContext();
        auto *state = ic->propertyFor(&factory_);
        if (event.type() == fcitx::EventType::InputContextFocusOut) {
            FfiKeyResult flushed = viet_ime_context_flush(state->ctx());
            applyResultToContext(ic, flushed);
            viet_ime_free_result(flushed);
        }
        reset(entry, event);
    }

    void keyEvent(const fcitx::InputMethodEntry &,
                  fcitx::KeyEvent &keyEvent) override {
        auto *ic = keyEvent.inputContext();
        auto *state = ic->propertyFor(&factory_);

        viet_ime_context_set_program(state->ctx(), ic->program().c_str());

        FfiKeyResult res = viet_ime_context_process_key(
            state->ctx(),
            static_cast<uint32_t>(keyEvent.key().sym()),
            static_cast<uint32_t>(keyEvent.key().states()),
            keyEvent.isRelease());

        applyResultToContext(ic, res);

        if (res.handled) {
            keyEvent.filterAndAccept();
        }

        viet_ime_free_result(res);
    }

    void reset(const fcitx::InputMethodEntry &,
               fcitx::InputContextEvent &event) override {
        auto *ic = event.inputContext();
        auto *state = ic->propertyFor(&factory_);
        viet_ime_context_reset(state->ctx());
        updatePreeditPanel(ic, nullptr, false);
    }

    void reloadConfig() override {
        instance_->inputContextManager().foreach([this](fcitx::InputContext *ic) {
            auto *state = ic->propertyFor(&factory_);
            viet_ime_context_reload_config(state->ctx());
            return true;
        });
    }

private:
    static void applyResultToContext(fcitx::InputContext *ic, const FfiKeyResult &res) {
        if (res.direct_mode) {
            updatePreeditPanel(ic, nullptr, false);

            if (res.backspace_count > 0) {
                if (ic->capabilityFlags().test(fcitx::CapabilityFlag::SurroundingText)) {
                    ic->deleteSurroundingText(
                        -static_cast<int>(res.backspace_count),
                        res.backspace_count);
                } else {
                    const fcitx::Key bsKey(FcitxKey_BackSpace);
                    for (uint32_t i = 0; i < res.backspace_count; ++i) {
                        ic->forwardKey(bsKey, false);
                        ic->forwardKey(bsKey, true);
                    }
                }
            }

            if (res.commit_delay_ms > 0 && (res.backspace_count > 0 || res.commit_text)) {
                std::this_thread::sleep_for(
                    std::chrono::milliseconds(res.commit_delay_ms));
            }

            if (res.commit_text && res.commit_text[0] != '\0') {
                ic->commitString(res.commit_text);
            }
        } else {
            if (res.commit_delay_ms > 0 && res.commit_text) {
                std::this_thread::sleep_for(
                    std::chrono::milliseconds(res.commit_delay_ms));
            }

            if (res.commit_text && res.commit_text[0] != '\0') {
                ic->commitString(res.commit_text);
            }

            updatePreeditPanel(ic, res.preedit_text, res.underline_preedit);
        }
    }

    static void updatePreeditPanel(fcitx::InputContext *ic,
                                   const char *preedit_str,
                                   bool underline) {
        auto &panel = ic->inputPanel();
        panel.reset();

        if (preedit_str && preedit_str[0] != '\0') {
            auto fmt = underline ? fcitx::TextFormatFlag::Underline
                                 : fcitx::TextFormatFlag::NoFlag;
            fcitx::Text preedit(preedit_str, fmt);
            preedit.setCursor(static_cast<int>(std::strlen(preedit_str)));
            if (ic->capabilityFlags().test(fcitx::CapabilityFlag::Preedit)) {
                panel.setClientPreedit(preedit);
            } else {
                panel.setPreedit(preedit);
            }
        }

        ic->updatePreedit();
        ic->updateUserInterface(fcitx::UserInterfaceComponent::InputPanel);
    }

    fcitx::Instance *instance_;
    fcitx::FactoryFor<TreGaiState> factory_;
};

class TreGaiAddonFactory final : public fcitx::AddonFactory {
public:
    fcitx::AddonInstance *create(fcitx::AddonManager *manager) override {
        return new TreGaiEngine(manager->instance());
    }
};

} // namespace

extern "C" void *rust_fcitx5_addon_factory_instance() {
    static TreGaiAddonFactory factory;
    return &factory;
}

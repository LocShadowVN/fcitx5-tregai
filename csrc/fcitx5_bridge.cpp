// SPDX-License-Identifier: GPL-3.0-or-later

#include <chrono>
#include <cstdint>
#include <cstring>
#include <fcitx-utils/capabilityflags.h>
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
    bool underline_preedit;
    bool direct_mode;
    uint32_t commit_delay_ms;
};

VietImeContext *viet_ime_context_new();
void viet_ime_context_free(VietImeContext *ctx);
void viet_ime_context_reset(VietImeContext *ctx);
void viet_ime_context_reload_config(VietImeContext *ctx);
FfiKeyResult viet_ime_context_flush(VietImeContext *ctx);
FfiKeyResult viet_ime_context_process_key(VietImeContext *ctx,
                                          uint32_t keysym,
                                          uint32_t state,
                                          bool is_release);
void viet_ime_free_result(FfiKeyResult res);
}

namespace {

static size_t countUtf8Codepoints(const std::string &s) {
    size_t count = 0;
    for (unsigned char c : s) {
        if ((c & 0xC0) != 0x80) {
            ++count;
        }
    }
    return count;
}

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
    size_t &directCommittedChars() { return direct_committed_chars_; }

private:
    VietImeContext *ctx_;
    size_t direct_committed_chars_ = 0;
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
        state->directCommittedChars() = 0;
        viet_ime_context_reset(state->ctx());
    }

    void deactivate(const fcitx::InputMethodEntry &entry,
                    fcitx::InputContextEvent &event) override {
        auto *ic = event.inputContext();
        auto *state = ic->propertyFor(&factory_);
        if (event.type() == fcitx::EventType::InputContextFocusOut) {
            FfiKeyResult flushed = viet_ime_context_flush(state->ctx());
            if (!flushed.direct_mode && flushed.commit_text && flushed.commit_text[0] != '\0') {
                ic->commitString(flushed.commit_text);
            }
            state->directCommittedChars() = 0;
            viet_ime_free_result(flushed);
        }
        reset(entry, event);
    }

    void keyEvent(const fcitx::InputMethodEntry &,
                  fcitx::KeyEvent &keyEvent) override {
        auto *ic = keyEvent.inputContext();
        auto *state = ic->propertyFor(&factory_);

        FfiKeyResult res = viet_ime_context_process_key(
            state->ctx(),
            static_cast<uint32_t>(keyEvent.key().sym()),
            static_cast<uint32_t>(keyEvent.key().states()),
            keyEvent.isRelease());

        if (res.commit_delay_ms > 0 && (res.commit_text || res.preedit_text)) {
            std::this_thread::sleep_for(
                std::chrono::milliseconds(res.commit_delay_ms));
        }

        const bool supports_surrounding =
            ic->capabilityFlags().test(fcitx::CapabilityFlag::SurroundingText);

        if (res.direct_mode && supports_surrounding) {
            updatePreeditPanel(ic, nullptr, false);

            if (res.commit_text && res.commit_text[0] != '\0') {
                if (state->directCommittedChars() > 0) {
                    ic->deleteSurroundingText(
                        -static_cast<int>(state->directCommittedChars()),
                        static_cast<unsigned int>(state->directCommittedChars()));
                }
                ic->commitString(res.commit_text);
                state->directCommittedChars() = 0;
            } else if (res.preedit_text) {
                std::string current_word(res.preedit_text);
                if (state->directCommittedChars() > 0) {
                    ic->deleteSurroundingText(
                        -static_cast<int>(state->directCommittedChars()),
                        static_cast<unsigned int>(state->directCommittedChars()));
                }
                if (!current_word.empty()) {
                    ic->commitString(current_word);
                }
                state->directCommittedChars() = countUtf8Codepoints(current_word);
            } else if (res.handled && state->directCommittedChars() > 0) {
                ic->deleteSurroundingText(
                    -static_cast<int>(state->directCommittedChars()),
                    static_cast<unsigned int>(state->directCommittedChars()));
                state->directCommittedChars() = 0;
            }
        } else {
            state->directCommittedChars() = 0;
            if (res.commit_text && res.commit_text[0] != '\0') {
                ic->commitString(res.commit_text);
            }
            updatePreeditPanel(ic, res.preedit_text, res.underline_preedit);
        }

        if (res.handled) {
            keyEvent.filterAndAccept();
        }

        viet_ime_free_result(res);
    }

    void reset(const fcitx::InputMethodEntry &,
               fcitx::InputContextEvent &event) override {
        auto *ic = event.inputContext();
        auto *state = ic->propertyFor(&factory_);
        state->directCommittedChars() = 0;
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

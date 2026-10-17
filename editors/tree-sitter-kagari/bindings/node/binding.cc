#include <napi.h>

typedef struct TSLanguage TSLanguage;
extern "C" TSLanguage *tree_sitter_kagari();

// The tree-sitter 0.25 Node binding checks this upstream language type tag.
static const napi_type_tag LANGUAGE_TYPE_TAG = {
    0x8AF2E5212AD58ABF, 0xD5006CAD83ABBA16
};

static Napi::Object Init(Napi::Env env, Napi::Object exports) {
    auto language = Napi::External<TSLanguage>::New(env, tree_sitter_kagari());
    language.TypeTag(&LANGUAGE_TYPE_TAG);
    exports["name"] = Napi::String::New(env, "kagari");
    exports["language"] = language;
    return exports;
}

NODE_API_MODULE(tree_sitter_kagari_binding, Init)

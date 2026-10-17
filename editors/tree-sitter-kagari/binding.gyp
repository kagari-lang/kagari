{
  "targets": [{
    "target_name": "tree_sitter_kagari_binding",
    "include_dirs": [
      "<!(node -p \"require('node:path').dirname(require.resolve('node-addon-api/package.json'))\")",
      "src"
    ],
    "defines": ["NAPI_CPP_EXCEPTIONS"],
    "cflags!": ["-fno-exceptions"],
    "cflags_cc!": ["-fno-exceptions"],
    "sources": ["bindings/node/binding.cc", "src/parser.c", "src/scanner.c"],
    "conditions": [
      ["OS!='win'", {"cflags_c": ["-std=c11"]}, {
        "cflags_c": ["/std:c11", "/utf-8"],
        "defines!": ["_HAS_EXCEPTIONS=0"],
        "defines": ["_HAS_EXCEPTIONS=1"],
        "msvs_settings": {"VCCLCompilerTool": {"ExceptionHandling": 1}}
      }],
      ["OS=='mac'", {"xcode_settings": {"GCC_ENABLE_CPP_EXCEPTIONS": "YES"}}]
    ]
  }]
}

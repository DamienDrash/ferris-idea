import js from "@eslint/js";

export default [
  js.configs.recommended,
  {
    languageOptions: { globals: { console: "readonly" } },
    rules: { "prefer-const": "error", eqeqeq: "warn" },
  },
];

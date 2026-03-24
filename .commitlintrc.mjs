export default {
  extends: ['@commitlint/config-conventional'],
  rules: {
    // Allow free-form scopes (any scope is valid)
    'scope-enum': [0],
  },
};

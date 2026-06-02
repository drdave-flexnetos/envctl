```markdown
# envctl Development Patterns

> Auto-generated skill from repository analysis

## Overview
This skill teaches you the core development patterns, conventions, and workflows used in the `envctl` Rust codebase. You'll learn how to structure files, write imports and exports, and follow the project's commit and testing styles. This guide is designed to help contributors quickly get up to speed and maintain consistency across the repository.

## Coding Conventions

### File Naming
- Use **camelCase** for file names.
  - Example: `envManager.rs`, `configLoader.rs`

### Import Style
- Use **relative imports** within the codebase.
  - Example:
    ```rust
    mod configLoader;
    use crate::configLoader::Config;
    ```

### Export Style
- Use **named exports** for modules and functions.
  - Example:
    ```rust
    pub fn load_env() { /* ... */ }
    pub struct EnvManager { /* ... */ }
    ```

### Commit Messages
- No strict format, but most messages are freeform and often prefixed with `envctl`.
- Average commit message length: ~77 characters.
  - Example:
    ```
    envctl add support for loading .env files from custom directories
    ```

## Workflows

### Adding a New Feature
**Trigger:** When implementing a new feature in envctl  
**Command:** `/add-feature`

1. Create a new camelCase-named file for your feature.
2. Implement the feature using relative imports and named exports.
3. Write or update tests as needed (see Testing Patterns).
4. Commit your changes with a descriptive message, optionally prefixed with `envctl`.
5. Open a pull request for review.

### Fixing a Bug
**Trigger:** When fixing a bug in the codebase  
**Command:** `/fix-bug`

1. Locate the relevant file(s) using camelCase naming.
2. Apply your fix, maintaining relative import and named export conventions.
3. Add or update tests to cover the bug fix.
4. Commit with a clear message, e.g., `envctl fix panic when loading empty .env`.
5. Submit your changes for review.

### Refactoring Code
**Trigger:** When improving code structure or readability  
**Command:** `/refactor`

1. Identify code that needs refactoring.
2. Rename files or modules using camelCase if needed.
3. Update imports/exports to remain relative and named.
4. Ensure all tests still pass.
5. Commit with a message like `envctl refactor config loading logic`.

## Testing Patterns

- **Framework:** Unknown (Rust standard testing likely)
- **File Pattern:** Tests are typically found in files matching `*.test.ts` (may indicate some TypeScript interop or documentation error; for Rust, use `mod tests` in your `.rs` files).
- **Example:**
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn test_env_loading() {
          // test logic here
      }
  }
  ```

## Commands

| Command        | Purpose                                    |
|----------------|--------------------------------------------|
| /add-feature   | Start the workflow for adding a new feature|
| /fix-bug       | Start the workflow for fixing a bug        |
| /refactor      | Start the workflow for refactoring code    |
```

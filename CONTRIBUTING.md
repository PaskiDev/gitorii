# Contributing to Torii ⛩️

Thank you for your interest in contributing to Torii! 🎉

## 📋 Table of Contents

- [Code of Conduct](#code-of-conduct)
- [How to Contribute](#how-to-contribute)
- [Pull Request Process](#pull-request-process)
- [Style Guides](#style-guides)
- [Contribution License](#contribution-license)

## 📜 Code of Conduct

This project follows a simple code of conduct: **be respectful and constructive**.

- ✅ Constructive criticism and technical debate
- ✅ Help other contributors
- ✅ Accept feedback on your code
- ❌ Personal attacks or offensive language
- ❌ Spam or self-promotion
- ❌ Sharing others' private information

## 🚀 How to Contribute

### Report Bugs 🐛

If you find a bug, open an issue with:

```markdown
**Description**: Brief description of the problem
**Steps to reproduce**:
1. ...
2. ...
**Expected behavior**: What should happen
**Actual behavior**: What actually happens
**Environment**:
- OS: [e.g: Ubuntu 22.04, macOS 14, Windows 11]
- Torii version: [e.g: 0.1.0]
- Rust version: [output from `rustc --version`]
```

### Suggest Features ✨

For new features, open an issue with:

```markdown
**Feature**: Feature name
**Problem it solves**: Why it's needed
**Proposed solution**: How it should work
**Alternatives considered**: Other options you evaluated
**Use cases**: Examples of when you'd use it
```

### Contribute Code 💻

1. **Fork** the repository
2. **Create a branch** from `main`:
   ```bash
   torii branch feature/my-new-feature -c
   # or
   torii branch fix/my-bug-fix -c
   ```
3. **Develop** your code
4. **Add tests** if applicable
5. **Commit** your changes:
   ```bash
   torii save -am "feat: add configurable snapshot system"
   ```
6. **Push** to your fork:
   ```bash
   torii sync --push
   ```
7. **Open a Pull Request**

## 🔄 Pull Request Process

### Before Submitting

- [ ] Code compiles without warnings: `cargo build --all-targets`
- [ ] Tests pass: `cargo test`
- [ ] Code follows style guides: `cargo fmt` and `cargo clippy`
- [ ] You've added/updated tests if necessary
- [ ] You've updated documentation if necessary
- [ ] Your commit follows message conventions

### PR Template

```markdown
## Description
Brief description of changes

## Type of change
- [ ] Bug fix (change that fixes an issue)
- [ ] New feature (change that adds functionality)
- [ ] Breaking change (fix or feature that causes existing functionality to change)
- [ ] Documentation

## How has this been tested?
Describe the tests you ran

## Checklist
- [ ] My code follows the project's style guides
- [ ] I have performed a self-review of my code
- [ ] I have commented my code in hard-to-understand areas
- [ ] I have updated corresponding documentation
- [ ] My changes generate no new warnings
- [ ] I have added tests that prove my fix works or my feature is effective
- [ ] New and existing unit tests pass locally
```

### Review

- All PRs require review before merge
- CI must pass (build, tests, clippy, fmt)
- Requested changes must be applied
- Maintainer will do final merge

## 🎨 Style Guides

### Rust Code Style

We use standard Rust style:

```bash
# Format code
cargo fmt

# Linter
cargo clippy -- -D warnings
```

**Conventions**:
- Variable names: `snake_case`
- Type names: `PascalCase`
- Constants: `SCREAMING_SNAKE_CASE`
- Maximum line length: 100 characters
- Use `?` instead of `unwrap()` when possible
- Document public functions with `///`

### Commit Messages

We follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>

[optional body]

[optional footer]
```

**Types**:
- `feat`: New functionality
- `fix`: Bug fix
- `docs`: Documentation changes
- `style`: Formatting, semicolons, etc (no code changes)
- `refactor`: Code refactoring
- `perf`: Performance improvements
- `test`: Add tests
- `chore`: Build changes, tools, etc

**Examples**:
```bash
feat(snapshots): add automatic cleanup of old snapshots
fix(sync): fix race condition in mirror sync
docs(readme): update installation instructions
refactor(core): simplify change detection logic
```

### File Structure

```
torii/
├── src/
│   ├── main.rs              # Entry point
│   ├── cli.rs               # CLI argument parsing and command dispatch
│   ├── core.rs              # Core git operations (init, add, commit, push, pull, status)
│   ├── core_extensions.rs   # Extended operations (log, diff, branch, rebase, history)
│   ├── core_integrate.rs    # Smart merge/rebase integration
│   ├── core_tag.rs          # Tag operations
│   ├── scanner.rs           # Sensitive data scanner
│   ├── snapshot.rs          # Snapshot management (create, restore, stash)
│   ├── mirror.rs            # Multi-platform mirror sync
│   ├── remote.rs            # Remote repository management (API clients)
│   ├── integrate.rs         # Integration analysis helper
│   ├── tag.rs               # Tag utilities
│   ├── config.rs            # Torii configuration
│   ├── toriignore.rs        # .toriignore pattern matching
│   ├── ssh.rs               # SSH key detection
│   ├── error.rs             # Error types
│   ├── duration.rs          # Duration parsing
│   └── versioning/          # Semantic versioning and auto-tagging
├── docs/
└── Cargo.toml
```

## 📝 Contribution License

### ⚠️ IMPORTANT: Read this before contributing

By submitting a Pull Request, you agree that:

1. **Copyright Transfer**: 
   - You transfer the copyright of your contribution to "Torii Project"
   - This allows us to manage the project's license effectively
   - This allows us to offer commercial licenses if necessary

2. **DCO Certification** (Developer Certificate of Origin):
   ```
   By making a commit, I certify that:
   
   (a) The contribution was created in whole or in part by me and 
       I have the right to submit it under the project's license; or
   
   (b) The contribution is based upon previous work that, to the best 
       of my knowledge, is covered under an appropriate open source 
       license and I have the right under that license to submit that 
       work with modifications; or
   
   (c) The contribution was provided directly to me by someone who 
       certified (a), (b) or (c) and I have not modified it.
   ```

3. **You Retain**:
   - ✅ The right to use your contribution in other projects
   - ✅ Recognition in the CONTRIBUTORS.md file
   - ✅ The right to list your contribution in your portfolio

4. **Outbound License**:
   - Your contribution will be licensed under the same terms as the project
   - Currently: Torii Source-Available License (Non-Commercial Fork-Friendly)
   - Torii Project may relicense in the future (e.g., for commercial licenses)

### Sign Your Commits

Add this line at the end of each commit message:

```
Signed-off-by: Your Name <your.email@example.com>
```

Or use git with the `-s` option:

```bash
torii save -am "feat: my new feature

Signed-off-by: Your Name <your.email@example.com>"
```

### Why Copyright Transfer?

This allows us to:
- Enforce the license against violations
- Offer commercial licenses (fund development)
- Migrate to another license if necessary in the future
- Defend the project legally

**Examples of projects that do this**: Qt, MySQL, MongoDB, GitLab

## 🏆 Recognition

All contributors will be listed in:
- `CONTRIBUTORS.md` - List of all who have contributed
- Release notes - Mention in the changelog
- README (featured contributors)

## 💬 Community

- **Discussions**: GitHub Discussions
- **Chat**: [Discord/Matrix/Slack - TBD]
- **Email**: paski@paski.dev

## 🙏 Thanks

Thank you for contributing to Torii! Every PR, issue, and suggestion makes the project better.

---

**Remember**: Code is for solving problems. Keep contributions focused on making Torii more useful for everyone. 🚀

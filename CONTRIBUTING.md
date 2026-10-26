# Contributing to TPS-API-Reference-Implementations

## Introduction

The TPS-API-Reference-Implementations GitHub project is developing commercial-grade reference implementations
of services defined by the [GlobalPlatform](https://globalplatform.org) Trusted Platform Services Committee.

As an implementation of a set of standards under development, contribution can occur in different ways.

- For GlobalPlatform members, through participation in the Trusted Platform Services Committee, there is an
  opportunity to discuss potential implementation options and define new service specifications through the
  ongoing activities of the Committee and its Working Groups.
- For anyone, including GlobalPlatform members, participation can take place through the GitHub Pull Request
  mechanism, subject to the rules outlined in this document.

Because we are trying to develop production quality implementations of multiple standards, there is a strong
preference towards contributions that do not deviate from the standards in question, as well as for good
documentation, examples and test cases where appropriate.

Contributions, in the form of pull requests, will be reviewed by the project leader or an assigned reviewer. This
can take a while (we all have day jobs). Bug-fixes and small enhancements may well be accepted with little or no
discussion beyond the initial review. Large and/or complex changes are taken to the relevant Working Group within
GlobalPlatform for discussion, which can take some time.

## Contribution Guidelines

### Contribution by Pull Request

Contributions should be provided as Pull Requests on an up-to-date fork of the project. These Pull Requests should
be able to be auto-merged.

### Developer Sign-Off

Contributions are only accepted if they have been signed-off by the developer using a GPG key that has been made
known to the project owner.

In signing-off a contribution, the contributor confirming that the contribution is being made under the terms of the
[Developer Certificate of Origin v1.1](https://developercertificate.org), and under the MIT license which is used for
all code under this project.

*Code that has not been signed-off with a verified key will be rejected.*

### Expectations of Code

#### Programming Language

The majority of the code in the TPS API Reference Implementations is written in Rust, and we are
generally trying to stay reasonably up-to-date with resepect to Rust Compiler versions and crate dependencies.

There are certain API points where the TPS reference implementations, while they are written in Rust, provide
C callable APIs. Contributions in C or C++ which make use of these APIs are certainly of interest, and there is
rudimentary CMake support in the project to assist in building these.

Contributions in other programming languages are of interest mainly to the extent that they expose TPS Services
through APIs for those other languages, e.g. something like a Python binding to the TPS Client API might be of
interest.

#### Code Quality

Code should be of generally good quality. This means that:

- It should compile without warnings.
- Rust code should pass most Clippy linters.
- Exported functions should have Rustdoc documentation.
- There should usually be tests.
- There should usually be example code.
- Code should be generally well-structured, use comments appropriately (especially where complex constructions
  are used)
- Function names should be descriptive and ideally follow a "verb", "object" naming pattern.

Code which is intended to run in a `no_std` environment should be tested to ensure that it does so. In practice, code
may well need to make use of Rust features.

Code which uses `unsafe` — in practice the crates handling raw pointers across the `extern "C"`
boundary (`tps_client_api`, `tps_client_c`, `tps_connector`, `tps_client_common`,
`rot13_connector`) — should additionally be checked with Miri by running `./run_miri.sh` before
raising a pull request. Neither the compiler nor `cargo test` can detect aliasing or pointer
provenance violations, so `unsafe` code can compile without warnings and pass its tests while
still being undefined behaviour. See the Miri section of `README.md` for setup and for which
crates are covered.

Two consequences for tests covering `unsafe` code:

- A test that cannot run under Miri (for example because it spawns a subprocess) should be marked
  `#[cfg_attr(miri, ignore = "<reason>")]` so that the reason travels with the test.
- A test must reach the `unsafe` operation it is meant to cover. A test that only exercises the
  early-return paths of a pointer-handling function tells Miri nothing about the pointer
  handling.

It is accepted that early contributions of larger features may be incomplete or lacking in some of the above areas, 
and the maintainer has some discretion to consider whether to accept contributions where this is the case. The guiding
principle is that any contribution must provide some useful feature that is end-to-end usable and of good quality of
iteslf.

#### Embedded Targets

Contributions of support for different embedded targets are especially welcome. While it is not aimed only at
embedded platforms, embedded targets are an important use-case for the TPS APIs and services.

#### Copyright, Licensing and Dependencies

Where you modify an existing file, you must add your copyright to that file. Where you create a new file, the
file must include your copyright statement.

For contributions coming via companies and other legal entities, the contributor should determine for themselves what
copyright statement is required. For individuals, it is sufficient to add a single line following the pattern below to
the file. 

```
Copyright (C) <year>, <name>, <e-mail address>
```

Where code introduces new dependencies, these MUST be available under a permissive licence that imposes no
obligation beyond attribution. The following are accepted: MIT, Apache-2.0 (including the variant carrying the
LLVM exception), BSD-2-Clause, BSD-3-Clause, ISC, Zlib, BSL-1.0, 0BSD, CC0-1.0 and Unicode-3.0. Where a
dependency is offered under a choice of licences, at least one of them must appear in that list.

*Code which introduces a dependency under a reciprocal ("copyleft") licence will be rejected.* This includes
file-scoped reciprocal licences such as MPL-2.0, which do not affect this project's own code but do oblige
anyone shipping a binary to publish the source of the covered files. Where such a dependency is genuinely
unavoidable it may be admitted only as a named exception in `deny.toml`, recording which licence applies, which
component pulls it in, and what obligation is thereby incurred.

`cargo deny check licenses` is the normative test, and its configuration in `deny.toml` is the authoritative
list. A contribution that causes that check to fail will be rejected.


The purpose of this rule is to ensure that users of the project have certainty about the licensing conditions
for the project: everything the project itself publishes is MIT, and no dependency imposes a condition that an
MIT distribution cannot satisfy.

## Getting Started

Here are some references and guidance to help new contributors to get started.

### Git

You will need Git running on the machine that you use to host your local copy of your fork of the repository.

GitHub has a [guide to setting up and using Git](https://docs.github.com/en/get-started/quickstart/set-up-git)

We require git commits to be signed-off using `git commit -S`, so please check the section on configuring GPG.

### GitHub Account

You will need a GitHub account where you will create a [fork](https://docs.github.com/en/get-started/quickstart/fork-a-repo) of 
the TPS-API-Reference-Implementations repository. You make your contributions from this fork.

This account mught be personal or it might belong to a company or other organizational contributor. If you are
employed, please ensure that your employer allows you to make contributions - you are responsible for this under
the Developer Certificate of Origin.

> We strongly recommend that you take steps to secure your GitHub account. This should include:
> 
> - Protection of the account with a strong password and two-factor authentication.
> - Use an authenticated mechanism such as https or ssh when connecting to GitHub

GitHub has good information on [how to contribute to projects](https://docs.github.com/en/get-started/quickstart/contributing-to-projects)
which describes the use of [Pull Requests](https://docs.github.com/en/get-started/quickstart/contributing-to-projects#making-a-pull-request)

### GPG and e-mail

We ask contributors to use GPG to sign their commits. We also like to match GPG keys to the e-mail addresses used
by the key owners.

GitHub provides a [guide to adding a GPG key to your account](https://docs.github.com/en/authentication/managing-commit-signature-verification/adding-a-gpg-key-to-your-github-account).
You should ensure that they key you use is associated with the e-mail address you used to set-up your GitHub account.
If you follow the GitHub guide, this will be the case.

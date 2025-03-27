# Dev tool #

This tool provides a standard interface to manage [12
factor](https://12factor.net/) projects, regardless of the language or
framework used. The main goal is to allow you to run a standard set of commands
for common tasks, which *just work:tm:* is any supported repo, such as:
- Manage encrypted environment variables
- Run a command in an environment
- Start up a full webserver in an environment
- Connect to an environments database (WIP)


## What are environments? ##

When manage an application, your normally require multiple environment,
normally at least production, and development. On top of that, you'll probably
need some form of local environment, so that you can run and test your
application, with similar settings to production.

In environments, we attempt to capture all dependencies required to have your
application up and running. The two key components are **environment
variables** and **external packages**. The external packages used between each
environment is the same, and the only difference are the environment variables.

In order to run a command within the default local environment, you can use the
following:
```bash
dev run <command>
```

This will make sure that all packages are automatically installed, and all
environment variables are exported for your command.

Sometimes you may want to run a one-off command in your local system, but connected
to other production services (such as your database), or with production settings. To do this,
you can run the following:
```bash
dev run -e prd <command>
```

### External packages ###

Whenever you deploy your application to production, it's good practise to lock
down external package to specific versions so you're able to recreate it if
required. In development environments, you are often just left following a
(hopefully) documented set of instructions to get your local system (mostly) in
sync with production. These can be useful on a fresh system, but get
complicated quickly when working on multiple products with different versions
of packages required. Even worse, you might be gradually doing upgrades between
OS releases across your team, and having conflict between the new packages, and
the old one.

`dev` allows you to use various tools to maintain external packages:
- The [Nix package manager](https://nixos.org/) resolves this issue with system
  level packages, by downloading and making programs and libraries available
  while within `dev run` or `dev start`. This can be used to automatically set
  up a certain version of nodejs, python or rust tools.
- Your language-specific package manager, such as poetry, uv, or npm, can
  resolve this issue for all packages managed by the language's traditional
  package repository.

### Environment variables ###

This is a hard one, particularly when it comes to local development. It's not
uncommon to see a local .env file, manually copied between developers, across
questionable mediums. If you're anything like me, you're probably getting major
*there must be a better way!* syndrome.

In `dev`, we use instead manage environment variables the same way for
production and local development. We create an encrypted file per environment
that contains all environment variables required to run the application. This
allows you to track all changes in version control, as if they were regular
code changes. It also means you can include environment variable deployments
into CI/CD pipelines, rather than managing them manually.

To modify your environment variables using your default, you can use the
command:
```
dev config edit [-e env]
```

## Getting started ##
To set up the dev command in you repo, follow these steps in the root of your repo:
```
mkdir -p .dev
cp ~/.ssh/id_ed25519.pub .dev/developers
```

## Commands ##

### Run a command with environment variables ###

```sh
dev run [-e env] <command> [args...]
```

### Start the development environment ###

This runs the command configured to start up the main service for this
application. If you want to run multiple services, you can try configuring it
to run something like
[process-compose](https://github.com/F1bonacc1/process-compose).


```sh
dev start [-e env]
```

### Manage configuration ###

Edit the environment configuration:

```sh
dev config edit [-e env]
```

Export the configuration:

```sh
dev config export [-e env] [--format <format>]
```

Available formats: raw, json, docker.

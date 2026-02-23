# freenet-shared

Shared tools and contracts for working with freenet apps

# Using this in your repo

Config example (deploy.toml in workspace root):

```toml
[project]
id = "my-project-id"

# Static apps 
[app]
type = "static"
folder = "my-webroot"

# Dioxus apps
[app]
type = "dioxus"
package-id = "my-ui"
```

Install tool globally

```
cargo install --path deploy-tool
```

Run initial-web-deploy (will create the keys and web-container-contract):

```
deploy-tool initial-web-deploy
```

Deploy your ap:

```
deploy-tool app
```

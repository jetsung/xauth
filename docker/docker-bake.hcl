## https://docs.docker.com/build/bake/
## https://docs.docker.com/reference/cli/docker/buildx/bake/#set
## https://github.com/crazy-max/buildx#remote-with-local
## https://github.com/docker/metadata-action

variable "VERSION" {
  default = "latest"
}

## Special target: https://github.com/docker/metadata-action#bake-definition
target "docker-metadata-action" {}

target "_image" {
    inherits = ["docker-metadata-action"]
}

target "_common" {
    context = "."
    dockerfile = "docker/Dockerfile"
    platforms = ["linux/amd64"]
    args = {
      VERSION = "${VERSION}"
    }
}

target "default" {
    inherits = ["_common"]
    tags = [
      "app:local",
    ]
}

group "dev" {
  targets = ["dev"]
}

target "dev" {
    inherits = ["_common", "_image"]
}

group "release" {
  targets = ["release"]
}

target "release" {
    inherits = ["_common", "_image"]
    platforms = ["linux/amd64","linux/arm64"]
}
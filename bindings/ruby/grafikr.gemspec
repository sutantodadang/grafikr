Gem::Specification.new do |s|
  s.name = "grafikr-ui"
  s.version = "1.1.0"
  s.summary = "Ruby desktop client for Grafikr"
  s.authors = ["Grafikr"]
  s.license = "AGPL-3.0-only"
  s.required_ruby_version = ">= 3.4"
  s.files = Dir["lib/**/*.rb", "examples/**/*.rb", "README.md", "runtime/**/*", "LICENSE"]
  s.require_paths = ["lib"]
end

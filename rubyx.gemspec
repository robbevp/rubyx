require_relative 'lib/rubyx/version'

Gem::Specification.new do |spec|
  spec.name          = "rubyx"
  spec.version       = Rubyx::VERSION
  spec.author        = "Naiker"
  spec.email         = ["yinho999@gmail.com"]
  spec.summary       = "Ruby-Python bridge powered by Rust"
  spec.required_ruby_version = Gem::Requirement.new(">= 3.0.0")

  spec.files         = Dir["lib/**/*.rb","ext/**/*.{rb,rs,toml}","Cargo.toml"]
  spec.require_paths = ["lib"]
  spec.extensions = ['ext/rubyx/extconf.rb']

  spec.add_dependency 'rb_sys', '~> 0.9'
  spec.add_development_dependency 'rake-compiler', '~> 1.2'
  spec.add_development_dependency 'rspec', '~> 3.0'

end
require 'mkmf'

unless respond_to?(:dummy_makefile)
  def dummy_makefile(dir)
    ["SHELL = /bin/sh",
     "ECHO = @echo",
     "Q = @",
     "TOUCH = touch",
     "COPY = cp",
     "RM_RF = rm -rf",
     "MAKEDIRS = mkdir -p",
     "INSTALL_PROG = install -m 0755",
     "all install static install-so install-rb:",
     "pre-install-rb: install-rb",
     "clean distclean realclean:",
     "\t@-$(RM_RF) mkmf.log"]
  end
end

require 'rb_sys/mkmf'

create_rust_makefile('rubyx/rubyx')
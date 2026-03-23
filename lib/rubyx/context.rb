module Rubyx
  class Context
    def eval(code, **globals)
      if globals.empty?
        _eval(code.to_s)
      else
        _eval_with_globals(code.to_s, globals)
      end
    end

    def await(code, **globals)
      if globals.empty?
        _await(code.to_s)
      else
        _await_with_globals(code.to_s, globals)
      end
    end

    def async_await(code, **globals)
      if globals.empty?
        _async_await(code.to_s)
      else
        _async_await_with_globals(code.to_s, globals)
      end
    end
  end
end

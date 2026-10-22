def usage
  <<~TEXT
    it's the usage
    # not a comment
  TEXT
end

def generated
  <<RUBY
    x = <<~RUBY
      it's generated
    RUBY
    # part of the generated code
RUBY
end
# a real comment

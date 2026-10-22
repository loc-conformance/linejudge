class Report
  class <<self
    # the class methods
    def title
      'Totals'
    end
  end

  def body
    <<TEXT
it's the body
# not a comment
TEXT
  end
  # a real comment
end

module Demo
  class Store
    def merge(items)
      added = 0
      items.each do |key, value|
        unless @items.key?(key)
          @items[key] = value
          added += 1
        end
      end
      added
    end
  end

  def self.collect_names(rows)
    normalize = ->(row) do
      name = row[:name].to_s.strip
      if name.empty?
        nil
      else
        name
      end
    end
    rows.filter_map do |row|
      normalize.call(row)
    end
  end
end

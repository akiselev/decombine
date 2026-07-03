<?php

namespace Demo;

class Store
{
    public function merge(array $items): int
    {
        $added = 0;
        foreach ($items as $key => $value) {
            if (!array_key_exists($key, $this->items)) {
                $this->items[$key] = $value;
                $added++;
            }
        }
        return $added;
    }
}

function collectNames(array $rows): array
{
    $normalize = function (array $row): ?string {
        if (!isset($row['name'])) {
            return null;
        }
        $name = trim($row['name']);
        return $name === '' ? null : $name;
    };
    $names = [];
    foreach ($rows as $row) {
        $name = $normalize($row);
        if ($name !== null) {
            $names[] = $name;
        }
    }
    return $names;
}

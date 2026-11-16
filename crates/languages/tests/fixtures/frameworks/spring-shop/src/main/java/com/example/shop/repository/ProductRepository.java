package com.example.shop.repository;

import com.example.shop.model.Product;
import java.util.ArrayList;
import java.util.List;
import org.springframework.stereotype.Repository;

@Repository
public class ProductRepository {
    private final List<Product> all = new ArrayList<>();

    public List<Product> findAll() {
        return all;
    }
}

package com.example.shop.service;

import com.example.shop.model.Product;
import com.example.shop.repository.ProductRepository;
import com.example.shop.tax.TaxCalculator;
import java.util.List;
import org.springframework.beans.factory.annotation.Value;
import org.springframework.stereotype.Service;

@Service
public class ProductService {
    private final ProductRepository repository;
    private final TaxCalculator tax;

    // Defined in application.properties.
    @Value("${shop.name}")
    private String shopName;

    // Defined in application.yml.
    @Value("${shop.currency}")
    private String currency;

    // Defined only in application-dev.yml (profile).
    @Value("${shop.debug-banner}")
    private String banner;

    // Kebab-case key, written in camel case in application.yml.
    @Value("${shop.max-items}")
    private int maxItems;

    // Defined nowhere, no default.
    @Value("${shop.missing-key}")
    private String missing;

    // Defined nowhere, with a default.
    @Value("${shop.timeout:30}")
    private int timeout;

    // A JVM system property and a random value: never in the files.
    @Value("${user.home}")
    private String home;

    @Value("${random.uuid}")
    private String instance;

    public ProductService(ProductRepository repository, TaxCalculator tax) {
        this.repository = repository;
        this.tax = tax;
    }

    public List<Product> all() {
        return repository.findAll();
    }

    public Product one(long id) {
        return repository.findAll().get(0);
    }

    public List<Product> byName(String name) {
        return repository.findAll();
    }

    public Product save(Product product) {
        return product;
    }

    public void delete(long id) {
    }
}

package com.example.shop.web;

import com.example.shop.model.Product;
import com.example.shop.service.ProductService;
import java.util.List;
import org.springframework.http.HttpStatus;
import org.springframework.http.MediaType;
import org.springframework.web.bind.annotation.DeleteMapping;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.PatchMapping;
import org.springframework.web.bind.annotation.PathVariable;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.PutMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RequestMethod;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.ResponseStatus;
import org.springframework.web.bind.annotation.RestController;

@RestController
@RequestMapping("/api/products")
public class ProductController {

    private final ProductService products;

    // Constructor injection without @Autowired.
    public ProductController(ProductService products) {
        this.products = products;
    }

    @GetMapping
    public List<Product> all() {
        return products.all();
    }

    @GetMapping("/{id}")
    public Product one(@PathVariable("id") long id) {
        return products.one(id);
    }

    @GetMapping(produces = MediaType.APPLICATION_JSON_VALUE, params = "name")
    public List<Product> byName(@RequestParam("name") String name) {
        return products.byName(name);
    }

    @PostMapping(path = "/", consumes = "application/json")
    @ResponseStatus(HttpStatus.CREATED)
    public Product create(@RequestBody Product product) {
        return products.save(product);
    }

    @PutMapping(value = {"/{id}", "/update/{id}"})
    public Product update(
            @PathVariable("id") long id,
            @RequestBody Product product) {
        return products.save(product);
    }

    @PatchMapping(
            value = "/{id}/price",
            consumes = MediaType.APPLICATION_JSON_VALUE)
    public Product reprice(@PathVariable long id, @RequestBody Product product) {
        return products.save(product);
    }

    @DeleteMapping(path = "/{id}")
    public void delete(@PathVariable long id) {
        products.delete(id);
    }

    @RequestMapping(value = "/search", method = RequestMethod.GET)
    public List<Product> search(@RequestParam String q) {
        return products.byName(q);
    }

    @RequestMapping(path = "/bulk", method = {RequestMethod.POST, RequestMethod.PUT})
    public void bulk(@RequestBody List<Product> all) {
        all.forEach(products::save);
    }

    @RequestMapping("/ping")
    public String ping() {
        return "pong";
    }
}

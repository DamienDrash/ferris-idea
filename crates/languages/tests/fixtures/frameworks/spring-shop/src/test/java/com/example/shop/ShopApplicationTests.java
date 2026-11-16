package com.example.shop;

import com.example.shop.payment.PaymentGateway;
import com.example.shop.service.ProductService;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.boot.test.context.SpringBootTest;
import org.springframework.boot.test.context.TestConfiguration;
import org.springframework.context.annotation.Bean;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RestController;

@SpringBootTest
class ShopApplicationTests {

    @Autowired
    private ProductService products;

    @TestConfiguration
    static class Fakes {
        @Bean
        PaymentGateway fakeGateway() {
            return cents -> { };
        }
    }

    @RestController
    static class ProbeController {
        @GetMapping("/test-only")
        String probe() {
            return "x";
        }
    }
}

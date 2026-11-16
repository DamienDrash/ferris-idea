package com.example.shop.web;

import com.example.shop.audit.AuditLog;
import com.example.shop.model.Order;
import com.example.shop.payment.PaymentGateway;
import com.example.shop.repository.OrderRepository;
import com.example.shop.service.CheckoutUseCase;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.beans.factory.annotation.Qualifier;
import org.springframework.stereotype.Controller;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.PathVariable;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.ResponseBody;

@Controller
@RequestMapping(path = {"/orders", "/api/orders"})
public class OrderController {

    // Field injection.
    @Autowired
    private OrderRepository orders;

    // Ambiguous: two gateways, none primary.
    @Autowired
    private PaymentGateway gateway;

    // Qualified: picks one of the two.
    @Autowired
    @Qualifier("paypalGateway")
    private PaymentGateway paypal;

    // Missing: AuditLog is a plain class, nobody declares it as a bean.
    @Autowired
    private AuditLog audit;

    @Autowired
    private CheckoutUseCase checkout;

    @GetMapping("/{id}")
    @ResponseBody
    public Order one(@PathVariable long id) {
        return orders.findById(id).orElseThrow();
    }

    @PostMapping
    @ResponseBody
    public Order place(Order order) {
        return checkout.place(order);
    }

    @GetMapping("/health") @ResponseBody public String health() { return "ok"; }
}

import http from "k6/http";
import { check, sleep } from "k6";

export const options = {
  scenarios: {
    baseline: {
      executor: "constant-vus",
      vus: 10,
      duration: "30s",
    },
    burst: {
      executor: "ramping-vus",
      startVUs: 0,
      stages: [
        { duration: "10s", target: 25 },
        { duration: "20s", target: 50 },
        { duration: "10s", target: 0 },
      ],
    },
  },
  thresholds: {
    http_req_failed: ["rate<0.01"],
    http_req_duration: ["p(95)<300", "p(99)<800"],
  },
};

const BASE_URL = __ENV.BASE_URL || "http://127.0.0.1:3000";

export default function () {
  const resApi = http.get(`${BASE_URL}/api/v1/api`);
  check(resApi, {
    "api status 200": (r) => r.status === 200,
  });

  const resHealth = http.get(`${BASE_URL}/health`);
  check(resHealth, {
    "health status 200": (r) => r.status === 200,
  });

  sleep(0.1);
}

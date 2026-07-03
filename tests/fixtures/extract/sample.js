// Fixture: declarations, methods, arrows named from context, small bodies.

class Router {
  dispatch(request) {
    // find the matching route
    const route = this.routes.find((r) => r.path === request.path);
    if (!route) {
      return { status: 404 };
    }
    const response = route.handler(request);
    return { status: 200, body: response };
  }
}

const parseQuery = (raw) => {
  const pairs = raw.split("&");
  const result = {};
  for (const pair of pairs) {
    const [key, value] = pair.split("=");
    result[key] = decodeURIComponent(value);
  }
  return result;
};

exports.middleware = function (req, res, next) {
  const started = Date.now();
  res.on("finish", () => {
    const elapsed = Date.now() - started;
    log(req.method, req.url, elapsed);
  });
  next();
};

const tiny = () => 1;

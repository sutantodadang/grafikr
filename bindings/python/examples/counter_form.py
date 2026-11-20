from grafikr import App, Node, Window

with App() as app:
    window = Window.create(app, "Counter and form", 480, 320)
    label = Node.create(app, window, "Text", text="Count: 0")
    label.attach(window)
    button = Node.create(app, window, "Button", label="Add")
    button.attach(window)
    app.on(button.handle, "click", lambda _event: label.update(text="Count: 1"))

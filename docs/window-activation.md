# Activación al abrir archivos desde el escritorio

En Linux con Wayland, el gestor de archivos entrega a la aplicación recién iniciada un token
`XDG_ACTIVATION_TOKEN`. Ese token permite al compositor colocar la nueva ventana en primer plano
y en el espacio de trabajo activo sin saltarse la protección contra robo de foco.

Cuando Pdf ya está ejecutándose, el mecanismo de instancia única normalmente envía el archivo a
esa ventana mediante un socket y termina el segundo proceso. En Wayland eso descartaba el token:
la ventana existente recibía el PDF, pero niri y otros compositores podían dejarla en segundo
plano o en otro espacio de trabajo.

Si una apertura externa llega con `WAYLAND_DISPLAY` y un token de activación, Pdf conserva el
proceso recién lanzado y crea una ventana nueva. Eframe consume el token durante la creación de
la ventana, por lo que el compositor puede enfocarla y mostrar el PDF donde el usuario está
trabajando. Las aperturas sin token, X11, Windows y los lanzamientos con opciones conservan el
comportamiento anterior.

El archivo de escritorio mantiene `StartupNotify=true`, necesario para que Files y otros
lanzadores compatibles soliciten el token.

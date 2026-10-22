# Modo consola semiplegado

En el menú lateral del juego, **Modo consola** permite elegir:

- **Automático al semiplegar** (predeterminado): activa el panel cuando Android
  informa de una bisagra horizontal en estado `HALF_OPENED`. Al desplegar,
  vuelve a pantalla completa.
- **Activado**: permite probar la división en una pantalla grande, aunque esté
  plana. Si hay una bisagra horizontal, respeta su posición real.
- **Desactivado**: conserva los controles y la pantalla completa habituales.

La opción se conserva entre sesiones. La pantalla exterior o una ventana sin
espacio suficiente vuelven a la distribución normal. Una bisagra vertical que
separa la ventana tampoco activa la división: gira el dispositivo.

El juego queda arriba; abajo hay joystick WASD con diagonales, un área para
cámara/cursor, atacar, usar, saltar, agacharse, inventario, soltar y nueve casillas
de barra rápida. **Esc**, **Teclado** y **⋯** (menú del juego) están en la cabecera.
El área de cámara se convierte en un panel de cursor con selección por toque
cuando Minecraft muestra un menú.

El joystick y los botones usan las asignaciones predeterminadas de Minecraft.
Si se cambian dentro del juego, hay que conservar esas asignaciones para este
panel. El editor de controles existente sigue disponible en pantalla completa.

La integración usa WindowManager 1.5.1 y cambia el tamaño del contenedor de juego;
no quita ni vuelve a añadir la superficie gráfica. SDL recibe las dimensiones
del área de juego y el cursor se limita a ella. Los controles originales se
restauran al salir. Cada dedo conserva su propio control hasta soltarlo; un
cambio de postura, foco o estado de captura cancela las pulsaciones pendientes.
El panel respeta las áreas de gestos del sistema y deja espacio junto al pliegue.

Desde `src-tauri/gen/android`, con JDK 21:

```powershell
.\gradlew.bat :game-runtime:testDebugUnitTest :game-runtime:testGraphicsAgent
```

Las pruebas cubren geometría, cancelación, botones simultáneos, joystick y cámara
simultáneos, selección por cursor y tamaños mínimos con gestos del sistema.
Las pruebas de vistas usan Android 16 simulado mediante Robolectric; no ejecutan
Minecraft ni prueban la GPU. Falta validar los cambios de postura durante una
partida real en el Fold 7, con OpenGL y Vulkan.

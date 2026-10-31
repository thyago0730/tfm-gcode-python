%
(G-CODE GERADO PELO TFM G-CODE)
(Data: 01/01/2000 00:00:00)
( SOFTWARE: TFM GCODE v11.3.2 )
(----------------------------------------)
( PROCEDIMENTO: Novo Procedimento )
( MODO DE SOLDAGEM: ESPIRAL )
( DIAMETRO: 50.000 mm )
( COMPRIMENTO: 50.000 mm )
( SOBREPOSICAO: 40.0% )
( VELOCIDADE DEP.: 120.0 mm/min )
( SENTIDO: esquerda_direita )
( SENTIDO ROTACAO: HORARIA )
( CAMADAS: 1 de 1.50mm )
( RESPOSTA TOCHA: LIGAR 3.00s / DESLIGAR 2.00s )
( LEAD-IN: 0.00 mm, LEAD-OUT: 0.00 mm )
(----------------------------------------)
G21 G90 G94

(PREPARO DA TOCHA)
G53 G0 Z0 Y0 (Ir ao Z&Y=0 maquina)
G90 G0 Y0 (Ir ao Y=0 peca)

(--- CAMADA 1 ---)
(SENTIDO: esquerda_direita)
G00 Z42.000
G00 X0.000 A0
G00 Z12.000
M0 (Confirme e pressione Start)
M101 (LIGAR Output#1 - tocha)
G04 P3.00 (Aguardar abertura da tocha)
G01 F274.9 X50.000 A3740.053
M102 (DESLIGAR Output#1 - tocha)
(APAGAMENTO DA TOCHA: manter cordao por 2.00s)
G01 F274.9 X50.122 A3749.217
G00 Z42.000


M102 (DESLIGAR Output#1 - tocha)
(RETORNO AO ZERO MAQUINA)
G90
G53 G0 Z0 Y0 (Ir ao Z&Y=0 maquina)
G53 G0 X0 (Ir ao X=0 maquina)
M30 (FIM DO PROGRAMA)
%